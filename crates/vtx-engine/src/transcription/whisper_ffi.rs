//! FFI bindings to whisper.cpp for all platforms.
//! This module uses libloading to dynamically load the whisper shared library at runtime.
//!
//! On Windows: whisper.dll is downloaded from GitHub releases
//! On macOS: libwhisper.dylib is downloaded from GitHub releases  
//! On Linux: libwhisper.so is built from source using CMake

use libloading::Library;
use std::ffi::{c_char, c_float, c_int, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Opaque pointer to whisper_context
type WhisperContext = *mut std::ffi::c_void;

/// Callback types (function pointers, nullable)
type WhisperNewSegmentCallback = *const std::ffi::c_void;
type WhisperProgressCallback = *const std::ffi::c_void;
type WhisperEncoderBeginCallback = *const std::ffi::c_void;
type WhisperAbortCallback = *const std::ffi::c_void;
type WhisperLogitsFilterCallback = *const std::ffi::c_void;
type WhisperGrammarElement = *const std::ffi::c_void;

/// VAD parameters struct
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WhisperVadParams {
    pub threshold: c_float,
    pub min_speech_duration_ms: c_int,
    pub min_silence_duration_ms: c_int,
    pub max_speech_duration_s: c_float,
    pub speech_pad_ms: c_int,
    pub samples_overlap: c_float,
}

/// whisper_full_params matching the C struct layout from whisper.h
/// IMPORTANT: This must match the exact layout of whisper_full_params in whisper.cpp
#[repr(C)]
#[derive(Clone)]
pub struct WhisperFullParams {
    pub strategy: c_int, // enum whisper_sampling_strategy

    pub n_threads: c_int,
    pub n_max_text_ctx: c_int,
    pub offset_ms: c_int,
    pub duration_ms: c_int,

    pub translate: bool,
    pub no_context: bool,
    pub no_timestamps: bool,
    pub single_segment: bool,
    pub print_special: bool,
    pub print_progress: bool,
    pub print_realtime: bool,
    pub print_timestamps: bool,

    // Token-level timestamps
    pub token_timestamps: bool,
    pub thold_pt: c_float,
    pub thold_ptsum: c_float,
    pub max_len: c_int,
    pub split_on_word: bool,
    pub max_tokens: c_int,

    // Speed-up techniques
    pub debug_mode: bool,
    pub audio_ctx: c_int,

    // Tinydiarize
    pub tdrz_enable: bool,

    // Suppress regex
    pub suppress_regex: *const c_char,

    // Initial prompt
    pub initial_prompt: *const c_char,
    pub carry_initial_prompt: bool,
    pub prompt_tokens: *const c_int,
    pub prompt_n_tokens: c_int,

    // Language
    pub language: *const c_char,
    pub detect_language: bool,

    // Decoding parameters
    pub suppress_blank: bool,
    pub suppress_nst: bool,

    pub temperature: c_float,
    pub max_initial_ts: c_float,
    pub length_penalty: c_float,

    // Fallback parameters
    pub temperature_inc: c_float,
    pub entropy_thold: c_float,
    pub logprob_thold: c_float,
    pub no_speech_thold: c_float,

    // Greedy params
    pub greedy_best_of: c_int,

    // Beam search params
    pub beam_search_beam_size: c_int,
    pub beam_search_patience: c_float,

    // Callbacks
    pub new_segment_callback: WhisperNewSegmentCallback,
    pub new_segment_callback_user_data: *mut std::ffi::c_void,

    pub progress_callback: WhisperProgressCallback,
    pub progress_callback_user_data: *mut std::ffi::c_void,

    pub encoder_begin_callback: WhisperEncoderBeginCallback,
    pub encoder_begin_callback_user_data: *mut std::ffi::c_void,

    pub abort_callback: WhisperAbortCallback,
    pub abort_callback_user_data: *mut std::ffi::c_void,

    pub logits_filter_callback: WhisperLogitsFilterCallback,
    pub logits_filter_callback_user_data: *mut std::ffi::c_void,

    // Grammar
    pub grammar_rules: *const WhisperGrammarElement,
    pub n_grammar_rules: usize,
    pub i_start_rule: usize,
    pub grammar_penalty: c_float,

    // VAD
    pub vad: bool,
    pub vad_model_path: *const c_char,
    pub vad_params: WhisperVadParams,
}

impl WhisperFullParams {
    /// Configure parameters optimized for short audio segments (real-time transcription)
    /// - n_samples: total samples in the (possibly padded) audio buffer
    /// - duration_ms: actual speech duration in milliseconds (before padding)
    #[allow(dead_code)]
    pub fn configure_for_short_audio(&mut self, n_samples: usize, duration_ms: c_int) {
        // Don't use past transcription as context (important for streaming)
        self.no_context = true;
        // Force single segment output (reduces latency)
        self.single_segment = true;
        // Suppress blank outputs
        self.suppress_blank = true;
        // Disable timestamps for speed
        self.no_timestamps = true;
        // Disable printing
        self.print_special = false;
        self.print_progress = false;
        self.print_realtime = false;
        self.print_timestamps = false;

        // Limit processing to actual speech duration (+ small buffer for safety)
        // This prevents whisper from hallucinating into padded silence
        self.duration_ms = duration_ms + 100;

        // Relax thresholds to accept lower-confidence results for short audio
        // Default logprob_thold (-1.0) is too strict for short segments
        self.logprob_thold = -2.0;
        // Disable entropy threshold - short repetitive audio (like "one two") has low entropy
        // Default entropy_thold (2.4) rejects valid short utterances
        self.entropy_thold = 0.0;
        // Disable temperature fallback - prefer fast results over retries
        self.temperature_inc = 0.0;
        // Limit max tokens to prevent repetition loops
        // For short audio (~1s), we expect at most ~10 tokens
        self.max_tokens = 16;

        // Optimize audio context size for actual audio length
        // audio_ctx is in mel spectrogram frames: n_samples / 160 (hop_length)
        // This can provide ~30% speedup for short audio
        let mel_frames = (n_samples / 160 + 1).min(1500) as c_int;
        self.audio_ctx = mel_frames;
    }

    /// Configure parameters with hallucination mitigation for transcription.
    ///
    /// This method applies settings that help prevent whisper from generating
    /// repetitive or hallucinated content, which is a known issue with the model.
    ///
    /// Key mitigations:
    /// - `no_context = true`: Prevents repetition from propagating between segments
    /// - `entropy_thold`: Filters uncertain/low-confidence outputs
    /// - `logprob_thold`: Filters low-probability token sequences
    /// - `no_speech_thold`: Better detection of actual silence vs hallucinated speech
    /// - `suppress_nst`: Suppresses non-speech tokens
    pub fn configure_with_hallucination_mitigation(&mut self) {
        // IMPORTANT: Disable cross-segment context to prevent repetition propagation
        // When true, each segment is transcribed independently without using the
        // previous segment's output as a prompt. This prevents a single repetition
        // from snowballing into massive loops across multiple segments.
        self.no_context = true;

        // Allow multiple output segments per chunk (for longer audio)
        self.single_segment = false;
        // Suppress blank outputs
        self.suppress_blank = true;
        // Enable timestamps for output
        self.no_timestamps = false;
        // Disable printing
        self.print_special = false;
        self.print_progress = false;
        self.print_realtime = false;
        self.print_timestamps = false;

        // Process full audio (no duration limit)
        self.duration_ms = 0;

        // Max tokens limit - 0 to disable and let whisper process all audio
        // Hallucination mitigation is handled by no_context=true and post-processing
        self.max_tokens = 0;

        // === Hallucination mitigation settings ===

        // Entropy threshold: segments with entropy above this are considered uncertain
        // Higher value = more aggressive filtering of uncertain outputs
        self.entropy_thold = 2.4;

        // Log probability threshold: segments with avg logprob below this are filtered
        // Higher (less negative) = more aggressive filtering
        self.logprob_thold = -0.8;

        // No-speech threshold: probability above which a segment is considered silence
        // Higher value = more likely to detect silence vs hallucinating content
        self.no_speech_thold = 0.6;

        // Suppress non-speech tokens (reduces hallucination of music/sounds as words)
        self.suppress_nst = true;

        // Temperature settings for fallback decoding
        // Start with deterministic decoding, increase on failure
        self.temperature = 0.0;
        self.temperature_inc = 0.2;

        // Length penalty to discourage very long outputs (hallucination mitigation)
        self.length_penalty = 1.0;
    }
}

/// Sampling strategy enum matching whisper.cpp
#[repr(C)]
#[allow(dead_code)]
pub enum WhisperSamplingStrategy {
    Greedy = 0,
    BeamSearch = 1,
}

/// Global library handle
static WHISPER_LIB: OnceLock<Option<WhisperLibrary>> = OnceLock::new();

/// Global ggml library handle (needed to load backends before whisper)
#[cfg(not(target_os = "macos"))]
static GGML_LIB: OnceLock<Option<GgmlLibrary>> = OnceLock::new();

/// Opaque pointer to ggml_backend_reg
#[cfg(not(target_os = "macos"))]
type GgmlBackendReg = *mut std::ffi::c_void;

/// Wrapper around the loaded ggml library (for backend loading)
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
struct GgmlLibrary {
    _lib: Library,
    backend_load_all_from_path: unsafe extern "C" fn(dir_path: *const c_char),
    backend_register: unsafe extern "C" fn(reg: GgmlBackendReg),
}

// SAFETY: The library handle and function pointers don't contain thread-local data
#[cfg(not(target_os = "macos"))]
unsafe impl Send for GgmlLibrary {}
#[cfg(not(target_os = "macos"))]
unsafe impl Sync for GgmlLibrary {}

#[cfg(not(target_os = "macos"))]
impl GgmlLibrary {
    /// Load the ggml library from the given path
    fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(path.as_ref())
                .map_err(|e| format!("Failed to load ggml library: {}", e))?;

            // Load ggml_backend_load_all_from_path - this function loads all backend plugins (CUDA, etc.)
            // from a specified directory
            let backend_load_all_from_path = *lib
                .get::<unsafe extern "C" fn(*const c_char)>(b"ggml_backend_load_all_from_path\0")
                .map_err(|e| format!("Failed to load ggml_backend_load_all_from_path: {}", e))?;

            // Load ggml_backend_register - used to manually register backends
            let backend_register = *lib
                .get::<unsafe extern "C" fn(GgmlBackendReg)>(b"ggml_backend_register\0")
                .map_err(|e| format!("Failed to load ggml_backend_register: {}", e))?;

            Ok(Self {
                _lib: lib,
                backend_load_all_from_path,
                backend_register,
            })
        }
    }

    /// Load all available backends (CUDA, etc.) from the specified directory
    fn load_backends_from_path(&self, dir_path: &Path) {
        let path_str = dir_path.to_string_lossy();
        let c_path = CString::new(path_str.as_ref()).unwrap_or_default();
        unsafe {
            (self.backend_load_all_from_path)(c_path.as_ptr());
        }
    }

    /// Register a backend manually
    #[allow(dead_code)]
    fn register_backend(&self, reg: GgmlBackendReg) {
        unsafe {
            (self.backend_register)(reg);
        }
    }
}

/// Try to load and register the CUDA backend manually
/// This is needed because the prebuilt ggml-cuda.dll doesn't follow the plugin convention
#[cfg(windows)]
#[allow(dead_code)]
fn try_load_cuda_backend(ggml_lib: &GgmlLibrary, lib_dir: &Path) {
    let cuda_dll_path = lib_dir.join("ggml-cuda.dll");
    if !cuda_dll_path.exists() {
        tracing::debug!("ggml-cuda.dll not found at {}", cuda_dll_path.display());
        return;
    }

    tracing::info!("Found ggml-cuda.dll, attempting to load CUDA backend...");

    unsafe {
        // Load ggml-cuda.dll
        let cuda_lib = match Library::new(&cuda_dll_path) {
            Ok(lib) => lib,
            Err(e) => {
                tracing::warn!("Failed to load ggml-cuda.dll: {}", e);
                return;
            }
        };

        // Get ggml_backend_cuda_reg function
        let cuda_reg_fn = match cuda_lib
            .get::<unsafe extern "C" fn() -> GgmlBackendReg>(b"ggml_backend_cuda_reg\0")
        {
            Ok(f) => *f,
            Err(e) => {
                tracing::warn!("Failed to find ggml_backend_cuda_reg: {}", e);
                return;
            }
        };

        // Call it to get the backend registration
        let cuda_reg = cuda_reg_fn();
        if cuda_reg.is_null() {
            tracing::warn!("ggml_backend_cuda_reg returned null");
            return;
        }

        // Register the CUDA backend with ggml
        tracing::info!("Registering CUDA backend...");
        ggml_lib.register_backend(cuda_reg);
        tracing::info!("CUDA backend registered successfully");

        // Keep the library loaded (leak it intentionally)
        std::mem::forget(cuda_lib);
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
#[allow(dead_code)]
fn try_load_cuda_backend(_ggml_lib: &GgmlLibrary, _lib_dir: &Path) {
    // On non-Windows, CUDA backend loading is handled differently
}

/// Wrapper around the loaded whisper library
pub struct WhisperLibrary {
    _lib: Library,
    // Function pointers
    init_from_file: unsafe extern "C" fn(path_model: *const c_char) -> WhisperContext,
    free: unsafe extern "C" fn(ctx: WhisperContext),
    full_default_params: unsafe extern "C" fn(strategy: c_int) -> WhisperFullParams,
    full: unsafe extern "C" fn(
        ctx: WhisperContext,
        params: WhisperFullParams,
        samples: *const c_float,
        n_samples: c_int,
    ) -> c_int,
    full_n_segments: unsafe extern "C" fn(ctx: WhisperContext) -> c_int,
    full_get_segment_text:
        unsafe extern "C" fn(ctx: WhisperContext, i_segment: c_int) -> *const c_char,
    print_system_info: unsafe extern "C" fn() -> *const c_char,
}

// SAFETY: The library handle and function pointers don't contain thread-local data
unsafe impl Send for WhisperLibrary {}
unsafe impl Sync for WhisperLibrary {}

impl WhisperLibrary {
    /// Load the whisper library from the given path
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(path.as_ref())
                .map_err(|e| format!("Failed to load whisper library: {}", e))?;

            // Load all required symbols - dereference immediately to get raw fn pointers
            let init_from_file = *lib
                .get::<unsafe extern "C" fn(*const c_char) -> WhisperContext>(
                    b"whisper_init_from_file\0",
                )
                .map_err(|e| format!("Failed to load whisper_init_from_file: {}", e))?;

            let free = *lib
                .get::<unsafe extern "C" fn(WhisperContext)>(b"whisper_free\0")
                .map_err(|e| format!("Failed to load whisper_free: {}", e))?;

            let full_default_params = *lib
                .get::<unsafe extern "C" fn(c_int) -> WhisperFullParams>(
                    b"whisper_full_default_params\0",
                )
                .map_err(|e| format!("Failed to load whisper_full_default_params: {}", e))?;

            let full = *lib
                .get::<unsafe extern "C" fn(
                    WhisperContext,
                    WhisperFullParams,
                    *const c_float,
                    c_int,
                ) -> c_int>(b"whisper_full\0")
                .map_err(|e| format!("Failed to load whisper_full: {}", e))?;

            let full_n_segments = *lib
                .get::<unsafe extern "C" fn(WhisperContext) -> c_int>(b"whisper_full_n_segments\0")
                .map_err(|e| format!("Failed to load whisper_full_n_segments: {}", e))?;

            let full_get_segment_text = *lib
                .get::<unsafe extern "C" fn(WhisperContext, c_int) -> *const c_char>(
                    b"whisper_full_get_segment_text\0",
                )
                .map_err(|e| format!("Failed to load whisper_full_get_segment_text: {}", e))?;

            let print_system_info = *lib
                .get::<unsafe extern "C" fn() -> *const c_char>(b"whisper_print_system_info\0")
                .map_err(|e| format!("Failed to load whisper_print_system_info: {}", e))?;

            Ok(Self {
                _lib: lib,
                init_from_file,
                free,
                full_default_params,
                full,
                full_n_segments,
                full_get_segment_text,
                print_system_info,
            })
        }
    }
}

/// On Windows, add a directory to the DLL search path.
/// This is necessary for whisper.dll to find its dependencies (ggml-cuda.dll, CUDA runtime, etc.)
/// when the service is run from a different working directory.
#[cfg(windows)]
fn add_dll_directory(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetDllDirectoryW(path: *const u16) -> i32;
    }

    // Convert path to wide string (null-terminated UTF-16)
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let result = unsafe { SetDllDirectoryW(wide.as_ptr()) };
    if result != 0 {
        tracing::debug!("Added DLL search directory: {}", dir.display());
    } else {
        tracing::warn!("Failed to add DLL search directory: {}", dir.display());
    }
}

/// Initialize the ggml library and load all backends (CUDA, etc.)
/// This must be called before loading whisper to ensure GPU backends are available.
/// On macOS, ggml is bundled inside libwhisper.dylib (Metal support), so we skip this.
fn init_ggml_backends() {
    // macOS: ggml is statically linked into libwhisper.dylib (Metal-enabled)
    // No separate ggml library needed - skip to avoid misleading warnings
    #[cfg(target_os = "macos")]
    {
        // Nothing to do on macOS
    }

    #[cfg(not(target_os = "macos"))]
    {
        GGML_LIB.get_or_init(|| {
            let lib_name = if cfg!(windows) {
                "ggml.dll"
            } else {
                "libggml.so"
            };

            // Search paths in order of preference
            let mut search_paths: Vec<PathBuf> = Vec::new();
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    search_paths.push(parent.join(lib_name));
                }
            }
            search_paths.push(std::env::current_dir().unwrap_or_default().join(lib_name));
            search_paths.extend(resource_dir_paths(lib_name));
            search_paths.push(std::path::PathBuf::from(lib_name));

            for path in search_paths.iter() {
                if path.exists() {
                    // Get the directory containing the library
                    let lib_dir = path.parent().map(|p| p.to_path_buf());

                    // On Windows, add the library's directory to DLL search path
                    // This allows ggml.dll to find ggml-cuda.dll and CUDA runtime
                    #[cfg(windows)]
                    if let Some(ref dir) = lib_dir {
                        add_dll_directory(dir);
                    }

                    match GgmlLibrary::load(path) {
                        Ok(lib) => {
                            tracing::info!("Loaded ggml library from: {}", path.display());
                            // Load all backends (CUDA, etc.) from the same directory - this is critical for GPU support
                            if let Some(ref dir) = lib_dir {
                                tracing::info!("Loading ggml backends from: {}", dir.display());
                                lib.load_backends_from_path(dir);

                                // Try to manually load CUDA backend (prebuilt binaries need this)
                                #[cfg(windows)]
                                try_load_cuda_backend(&lib, dir);

                                tracing::info!("ggml backends loaded");
                            }
                            return Some(lib);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to load ggml library from {}: {}",
                                path.display(),
                                e
                            );
                        }
                    }
                }
            }

            // Try loading from system path
            match GgmlLibrary::load(lib_name) {
                Ok(lib) => {
                    tracing::info!("Loaded ggml library from system path");
                    // Load backends from current directory as fallback
                    let cwd = std::env::current_dir().unwrap_or_default();
                    lib.load_backends_from_path(&cwd);
                    Some(lib)
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to load ggml library: {} - GPU backends may not be available",
                        e
                    );
                    None
                }
            }
        });
    }
}

/// Initialize the global whisper library
pub fn init_library() -> Result<(), String> {
    // First, load ggml and initialize backends (CUDA, etc.)
    // This must happen before loading whisper for GPU support to work
    init_ggml_backends();

    WHISPER_LIB.get_or_init(|| {
        // Try to find the library in various locations
        let lib_name = if cfg!(windows) {
            "whisper.dll"
        } else if cfg!(target_os = "macos") {
            "libwhisper.dylib"
        } else {
            "libwhisper.so"
        };

        // Search paths in order of preference:
        // 1. Next to the executable
        // 2. In the current directory
        // 3. In the bundled resource directory (Windows)
        // 3. macOS app bundle: Contents/Resources/ and Contents/Frameworks/
        // 4. System library paths (handled by libloading)
        let mut base_paths: Vec<PathBuf> = Vec::new();
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                base_paths.push(parent.join(lib_name));
            }
        }
        base_paths.push(std::env::current_dir().unwrap_or_default().join(lib_name));
        base_paths.extend(resource_dir_paths(lib_name));
        base_paths.push(std::path::PathBuf::from(lib_name));

        #[cfg(target_os = "macos")]
        let macos_paths: Vec<PathBuf> = if let Some(exe_path) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        {
            vec![
                // Contents/Resources/Frameworks/ - where Tauri bundles resources with subdirectories
                exe_path.join("../Resources/Frameworks").join(lib_name),
                // Contents/Resources/ - direct resource location
                exe_path.join("../Resources").join(lib_name),
                // Contents/Frameworks/ - standard macOS location
                exe_path.join("../Frameworks").join(lib_name),
            ]
        } else {
            vec![]
        };
        #[cfg(not(target_os = "macos"))]
        let macos_paths: Vec<PathBuf> = vec![];

        for path in base_paths.iter().chain(macos_paths.iter()) {
            if path.exists() {
                // On Windows, add the library's directory to DLL search path
                // This allows whisper.dll to find its dependencies (ggml-cuda.dll, CUDA runtime, etc.)
                #[cfg(windows)]
                if let Some(lib_dir) = path.parent() {
                    add_dll_directory(lib_dir);
                }

                match WhisperLibrary::load(path) {
                    Ok(lib) => {
                        tracing::info!("Loaded whisper library from: {}", path.display());
                        return Some(lib);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load whisper library from {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        // Try loading from system path
        match WhisperLibrary::load(lib_name) {
            Ok(lib) => {
                tracing::info!("Loaded whisper library from system path");
                Some(lib)
            }
            Err(e) => {
                tracing::error!("Failed to load whisper library: {}", e);
                None
            }
        }
    });

    if WHISPER_LIB.get().and_then(|l| l.as_ref()).is_some() {
        Ok(())
    } else {
        Err("Whisper library not available".to_string())
    }
}

fn resource_dir_paths(lib_name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(resource_dir) = std::env::var("VTX_RESOURCE_DIR") {
        if !resource_dir.is_empty() {
            let base = PathBuf::from(resource_dir);
            paths.push(base.join(lib_name));
            paths.push(base.join("binaries").join(lib_name));
        }
    }
    paths
}

/// Get the loaded library or return an error
fn get_lib() -> Result<&'static WhisperLibrary, String> {
    WHISPER_LIB
        .get()
        .and_then(|l| l.as_ref())
        .ok_or_else(|| "Whisper library not loaded".to_string())
}

/// Safe wrapper around whisper context
pub struct Context {
    ptr: WhisperContext,
}

// SAFETY: WhisperContext is thread-safe according to whisper.cpp documentation
unsafe impl Send for Context {}

impl Context {
    /// Create a new context from a model file
    pub fn new<P: AsRef<Path>>(model_path: P) -> Result<Self, String> {
        let lib = get_lib()?;

        let path_str = model_path.as_ref().to_str().ok_or("Invalid model path")?;
        let c_path = CString::new(path_str).map_err(|e| format!("Invalid path: {}", e))?;

        let ptr = unsafe { (lib.init_from_file)(c_path.as_ptr()) };

        if ptr.is_null() {
            return Err(format!(
                "Failed to initialize whisper context from: {}",
                path_str
            ));
        }

        Ok(Self { ptr })
    }

    /// Run full transcription on audio samples
    pub fn full(&self, params: &WhisperFullParams, samples: &[f32]) -> Result<(), String> {
        let lib = get_lib()?;

        let result = unsafe {
            (lib.full)(
                self.ptr,
                params.clone(),
                samples.as_ptr(),
                samples.len() as c_int,
            )
        };

        if result != 0 {
            return Err(format!("Transcription failed with code: {}", result));
        }

        Ok(())
    }

    /// Get the number of segments in the transcription result
    pub fn full_n_segments(&self) -> Result<i32, String> {
        let lib = get_lib()?;
        Ok(unsafe { (lib.full_n_segments)(self.ptr) })
    }

    /// Get the text of a specific segment
    pub fn full_get_segment_text(&self, i_segment: i32) -> Result<String, String> {
        let lib = get_lib()?;

        let ptr = unsafe { (lib.full_get_segment_text)(self.ptr, i_segment) };

        if ptr.is_null() {
            return Err(format!("Failed to get segment {} text", i_segment));
        }

        let c_str = unsafe { CStr::from_ptr(ptr) };
        c_str
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| format!("Invalid UTF-8 in segment: {}", e))
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if let Ok(lib) = get_lib() {
            unsafe { (lib.free)(self.ptr) };
        }
    }
}

/// Get default parameters for the given sampling strategy
pub fn full_default_params(strategy: WhisperSamplingStrategy) -> Result<WhisperFullParams, String> {
    let lib = get_lib()?;
    Ok(unsafe { (lib.full_default_params)(strategy as c_int) })
}

/// Get whisper.cpp system info string
/// This includes information about available backends (CPU, CUDA, Metal, etc.)
pub fn get_system_info() -> Result<String, String> {
    let lib = get_lib()?;
    let ptr = unsafe { (lib.print_system_info)() };
    if ptr.is_null() {
        return Err("Failed to get system info".to_string());
    }
    let c_str = unsafe { CStr::from_ptr(ptr) };
    c_str
        .to_str()
        .map(|s| s.to_string())
        .map_err(|e| format!("Invalid UTF-8 in system info: {}", e))
}
