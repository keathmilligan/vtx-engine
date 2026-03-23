//! vtx-demo: Demo application for the vtx-engine voice processing library.
//!
//! Provides a simple UI to test live audio capture, visualization, and
//! WAV file transcription.

use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;
use tracing::info;

use vtx_engine::*;
use vtx_engine::{AudioEngine, EngineBuilder, ModelManager, WhisperModel};

fn normalize_model_name(model: &str) -> Result<String, String> {
    WhisperModel::parse_identifier(model)
        .map(|parsed| parsed.config_key().to_string())
        .ok_or_else(|| format!("Invalid model name: {model}"))
}

fn parse_model_name(model: &str) -> Result<WhisperModel, String> {
    WhisperModel::parse_identifier(model).ok_or_else(|| format!("Invalid model name: {model}"))
}

/// Application state shared across Tauri commands.
struct AppState {
    engine: Arc<Mutex<Option<AudioEngine>>>,
    model_manager: ModelManager,
    download_handles: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

/// Model status for a single WhisperModel variant.
#[derive(Debug, Clone, serde::Serialize)]
struct ModelStatusEntry {
    model: String,
    name: String,
    size_mb: u32,
    downloaded: bool,
}

/// Demo app configuration persisted to JSON file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DemoConfig {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub transcription_enabled: bool,
    #[serde(default)]
    pub auto_transcription_enabled: bool,
    #[serde(default)]
    pub aec_enabled: bool,
    #[serde(default)]
    pub primary_device_id: String,
    #[serde(default)]
    pub secondary_device_id: String,
    #[serde(default)]
    pub mic_gain_db: f64,
    #[serde(default)]
    pub vad_voiced_threshold_db: f64,
    #[serde(default)]
    pub vad_whisper_threshold_db: f64,
    #[serde(default)]
    pub vad_voiced_onset_ms: u32,
    #[serde(default)]
    pub vad_whisper_onset_ms: u32,
    #[serde(default)]
    pub segment_max_duration_ms: u32,
    #[serde(default)]
    pub segment_word_break_grace_ms: u32,
    #[serde(default)]
    pub segment_lookback_ms: u32,
    #[serde(default = "default_transcription_queue_capacity")]
    pub transcription_queue_capacity: u32,
    #[serde(default = "default_viz_frame_interval_ms")]
    pub viz_frame_interval_ms: u32,
    #[serde(default = "default_word_break_segmentation_enabled")]
    pub word_break_segmentation_enabled: bool,
    #[serde(default)]
    pub audio_output_device_id: String,
    #[serde(default)]
    pub agc_enabled: bool,
    #[serde(default = "default_agc_target_level_db")]
    pub agc_target_level_db: f64,
    #[serde(default = "default_agc_gate_threshold_db")]
    pub agc_gate_threshold_db: f64,
}

fn default_transcription_queue_capacity() -> u32 {
    8
}
fn default_viz_frame_interval_ms() -> u32 {
    16
}
fn default_word_break_segmentation_enabled() -> bool {
    true
}
fn default_agc_target_level_db() -> f64 {
    -18.0
}
fn default_agc_gate_threshold_db() -> f64 {
    -50.0
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            model: "base_en".to_string(),
            transcription_enabled: true,
            auto_transcription_enabled: false,
            aec_enabled: false,
            primary_device_id: String::new(),
            secondary_device_id: String::new(),
            mic_gain_db: 0.0,
            vad_voiced_threshold_db: -42.0,
            vad_whisper_threshold_db: -52.0,
            vad_voiced_onset_ms: 80,
            vad_whisper_onset_ms: 120,
            segment_max_duration_ms: 4000,
            segment_word_break_grace_ms: 750,
            segment_lookback_ms: 200,
            transcription_queue_capacity: default_transcription_queue_capacity(),
            viz_frame_interval_ms: default_viz_frame_interval_ms(),
            word_break_segmentation_enabled: default_word_break_segmentation_enabled(),
            audio_output_device_id: String::new(),
            agc_enabled: false,
            agc_target_level_db: default_agc_target_level_db(),
            agc_gate_threshold_db: default_agc_gate_threshold_db(),
        }
    }
}

const CONFIG_FILENAME: &str = "config.json";

fn demo_config_path() -> Result<std::path::PathBuf, String> {
    let dirs = directories::ProjectDirs::from("", "", "vtx-demo")
        .ok_or("Cannot determine config directory")?;
    Ok(dirs.config_dir().join(CONFIG_FILENAME))
}

impl DemoConfig {
    pub fn load() -> Result<Self, String> {
        let path = demo_config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let mut config: Self = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        if !config.model.trim().is_empty() {
            config.model = normalize_model_name(&config.model)?;
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = demo_config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        let mut normalized = self.clone();
        if !normalized.model.trim().is_empty() {
            normalized.model = normalize_model_name(&normalized.model)?;
        }
        let content = serde_json::to_string_pretty(&normalized)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write config file: {}", e))
    }
}

// =============================================================================
// Tauri Commands
// =============================================================================

#[tauri::command]
async fn list_input_devices(state: tauri::State<'_, AppState>) -> Result<Vec<AudioDevice>, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.list_input_devices())
}

#[tauri::command]
async fn list_system_devices(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AudioDevice>, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.list_system_devices())
}

#[tauri::command]
async fn start_capture(
    state: tauri::State<'_, AppState>,
    source_id: String,
    source2_id: Option<String>,
    recording_mode: Option<String>,
) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    let _ = recording_mode; // Mode is set at construction; acknowledged here for API compat
    engine.start_capture(Some(source_id), source2_id).await
}

#[tauri::command]
async fn stop_capture(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.stop_capture().await
}

#[tauri::command]
async fn is_capturing(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.is_capturing())
}

#[tauri::command]
async fn check_model_status(state: tauri::State<'_, AppState>) -> Result<ModelStatus, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.check_model_status())
}

#[tauri::command]
async fn download_model(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.download_model().await
}

#[tauri::command]
async fn transcribe_file(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<Vec<vtx_engine::TranscriptionSegment>, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.transcribe_audio_file(path).await
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<EngineStatus, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.get_status())
}

#[tauri::command]
async fn get_gpu_status(state: tauri::State<'_, AppState>) -> Result<GpuStatus, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.check_gpu_status()
}

#[tauri::command]
async fn set_transcription_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.set_transcription_enabled(enabled);
    Ok(())
}

#[tauri::command]
async fn is_transcription_enabled(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.is_transcription_enabled())
}

#[tauri::command]
async fn start_recording(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.start_recording();
    Ok(())
}

#[tauri::command]
async fn stop_recording(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.stop_recording();
    let path = engine
        .get_last_recording_path()
        .map(|p| p.to_string_lossy().into_owned());
    Ok(path)
}

#[tauri::command]
async fn open_file(
    state: tauri::State<'_, AppState>,
    path: String,
    ptt_mode: bool,
) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.play_file(path, ptt_mode)
}

#[tauri::command]
async fn stop_playback(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.stop_playback();
    Ok(())
}

#[tauri::command]
async fn supports_render_output() -> Result<bool, String> {
    Ok(vtx_engine::platform::get_backend()
        .map(|backend| backend.supports_render_output())
        .unwrap_or(cfg!(target_os = "windows")))
}

#[tauri::command]
async fn is_recording(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.is_recording())
}

#[tauri::command]
async fn set_ptt_mode(state: tauri::State<'_, AppState>, enabled: bool) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.set_ptt_mode(enabled);
    Ok(())
}

#[tauri::command]
async fn finalize_segment(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.finalize_segment();
    Ok(())
}

#[tauri::command]
async fn get_engine_config(
    state: tauri::State<'_, AppState>,
) -> Result<vtx_engine::EngineConfig, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.config().clone())
}

#[tauri::command]
async fn set_engine_config(
    state: tauri::State<'_, AppState>,
    config: vtx_engine::EngineConfig,
) -> Result<(), String> {
    let mut engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_mut().ok_or("Engine not initialized")?;
    engine.set_config(config);
    Ok(())
}

#[tauri::command]
async fn get_model_status(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ModelStatusEntry>, String> {
    let manager = &state.model_manager;
    let status: Vec<ModelStatusEntry> = WhisperModel::all_in_size_order()
        .iter()
        .map(|&model| ModelStatusEntry {
            model: model.config_key().to_string(),
            name: model.display_name().to_string(),
            size_mb: model.size_mb(),
            downloaded: manager.is_available(model),
        })
        .collect();
    Ok(status)
}

#[tauri::command]
async fn download_model_by_name(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
) -> Result<(), String> {
    let model = parse_model_name(&model)?;
    let model_key = model.config_key().to_string();

    let manager = state.model_manager.clone();
    let handles = state.download_handles.clone();
    let app_handle = app.clone();
    let model_key_for_progress = model_key.clone();
    let model_key_for_complete = model_key.clone();
    let model_key_for_remove = model_key.clone();

    let handle = tokio::spawn(async move {
        let app_for_progress = app_handle.clone();
        let model_for_progress = model_key_for_progress.clone();
        let result = manager
            .download(model, move |progress| {
                let _ = app_for_progress.emit(
                    "model-download-progress",
                    serde_json::json!({
                        "model": model_for_progress,
                        "progress": progress
                    }),
                );
            })
            .await;

        match result {
            Ok(()) => {
                let _ = app_handle.emit(
                    "model-download-progress",
                    serde_json::json!({
                        "model": model_key_for_complete,
                        "progress": 100
                    }),
                );
            }
            Err(e) => {
                let _ = app_handle.emit(
                    "model-download-error",
                    serde_json::json!({
                        "model": model_key_for_complete,
                        "error": e.to_string()
                    }),
                );
            }
        }

        handles.lock().await.remove(&model_key_for_remove);
    });

    state
        .download_handles
        .lock()
        .await
        .insert(model_key, handle);
    Ok(())
}

#[tauri::command]
async fn cancel_model_download(
    state: tauri::State<'_, AppState>,
    model: String,
) -> Result<(), String> {
    let model = parse_model_name(&model)?;
    let model_key = model.config_key().to_string();

    if let Some(handle) = state.download_handles.lock().await.remove(&model_key) {
        handle.abort();
    }
    Ok(())
}

#[tauri::command]
async fn load_demo_config() -> Result<DemoConfig, String> {
    DemoConfig::load()
}

#[tauri::command]
async fn save_demo_config(config: DemoConfig) -> Result<(), String> {
    config.save()
}

// =============================================================================
// App Entry Point
// =============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vtx_engine=info".parse().unwrap())
                .add_directive("vtx_demo=info".parse().unwrap()),
        )
        .init();

    info!("vtx-demo starting...");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            let state = AppState {
                engine: Arc::new(Mutex::new(None)),
                model_manager: ModelManager::new("vtx-engine"),
                download_handles: Arc::new(Mutex::new(HashMap::new())),
            };

            let engine_arc = state.engine.clone();

            tauri::async_runtime::spawn(async move {
                match EngineBuilder::new().build().await {
                    Ok((engine, rx)) => {
                        info!("Audio engine initialized successfully");

                        // Spawn event forwarding task using EventHandlerAdapter pattern
                        let ah = app_handle.clone();
                        vtx_engine::EventHandlerAdapter::new(rx, move |event| match &event {
                            EngineEvent::VisualizationData(data) => {
                                let _ = ah.emit("visualization-data", data);
                            }
                            EngineEvent::TranscriptionComplete(result) => {
                                let _ = ah.emit("transcription-complete", result);
                            }
                            EngineEvent::SpeechStarted => {
                                let _ = ah.emit("speech-started", ());
                            }
                            EngineEvent::SpeechEnded { duration_ms } => {
                                let _ = ah.emit("speech-ended", duration_ms);
                            }
                            EngineEvent::CaptureStateChanged { capturing, error } => {
                                #[derive(serde::Serialize, Clone)]
                                struct CaptureState {
                                    capturing: bool,
                                    error: Option<String>,
                                }
                                let _ = ah.emit(
                                    "capture-state-changed",
                                    CaptureState {
                                        capturing: *capturing,
                                        error: error.clone(),
                                    },
                                );
                            }
                            EngineEvent::ModelDownloadProgress { percent } => {
                                let _ = ah.emit("model-download-progress", percent);
                            }
                            EngineEvent::ModelDownloadComplete { success } => {
                                let _ = ah.emit("model-download-complete", success);
                            }
                            EngineEvent::AudioLevelUpdate {
                                device_id,
                                level_db,
                            } => {
                                #[derive(serde::Serialize, Clone)]
                                struct LevelUpdate {
                                    device_id: String,
                                    level_db: f32,
                                }
                                let _ = ah.emit(
                                    "audio-level-update",
                                    LevelUpdate {
                                        device_id: device_id.clone(),
                                        level_db: *level_db,
                                    },
                                );
                            }
                            EngineEvent::TranscriptionSegment(seg) => {
                                let _ = ah.emit("transcription-segment", seg);
                            }
                            EngineEvent::RecordingStarted => {
                                let _ = ah.emit("recording-started", ());
                            }
                            EngineEvent::RecordingStopped { duration_ms } => {
                                let _ = ah.emit("recording-stopped", duration_ms);
                            }
                            EngineEvent::PlaybackComplete => {
                                let _ = ah.emit("playback-complete", ());
                            }
                            EngineEvent::AgcGainChanged(gain_db) => {
                                let _ = ah.emit("agc-gain-changed", gain_db);
                            }
                            EngineEvent::AudioData(data) => {
                                let _ = ah.emit("audio-data", data);
                            }
                            EngineEvent::RawAudioData(data) => {
                                let _ = ah.emit("raw-audio-data", data);
                            }
                        })
                        .spawn();

                        *engine_arc.lock().await = Some(engine);
                    }
                    Err(e) => {
                        tracing::error!("Failed to initialize audio engine: {}", e);
                    }
                }
            });

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_input_devices,
            list_system_devices,
            start_capture,
            stop_capture,
            is_capturing,
            check_model_status,
            download_model,
            transcribe_file,
            get_status,
            get_gpu_status,
            set_transcription_enabled,
            is_transcription_enabled,
            start_recording,
            stop_recording,
            is_recording,
            open_file,
            stop_playback,
            supports_render_output,
            get_engine_config,
            set_engine_config,
            set_ptt_mode,
            finalize_segment,
            get_model_status,
            download_model_by_name,
            cancel_model_download,
            load_demo_config,
            save_demo_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running vtx-demo");
}
