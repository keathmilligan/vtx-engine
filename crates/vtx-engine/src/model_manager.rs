//! Model management: availability checks, path resolution, and async download.
//!
//! [`ModelManager`] is a standalone public struct that does not require a
//! running [`AudioEngine`]. Both FlowSTT and OmniRec can use it during their
//! settings UIs / first-run setup, before any capture session is configured.
//!
//! # Example
//!
//! ```rust,no_run
//! use vtx_engine::ModelManager;
//! use vtx_engine::WhisperModel;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mgr = ModelManager::new("my-app");
//!     if !mgr.is_available(WhisperModel::BaseEn) {
//!         mgr.download(WhisperModel::BaseEn, |pct| println!("{}%", pct))
//!             .await
//!             .unwrap();
//!     }
//!     println!("model at: {}", mgr.path(WhisperModel::BaseEn).display());
//! }
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::WhisperModel;

// =============================================================================
// ModelError
// =============================================================================

/// Typed error variants for model management operations.
#[derive(Debug)]
pub enum ModelError {
    /// An I/O error occurred (file system operation failed).
    Io(std::io::Error),
    /// A network error occurred during download.
    Network(String),
    /// Could not determine the platform project/cache directory.
    NoProjectDir,
    /// A download for the same model is already in progress.
    AlreadyDownloading,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::Io(e) => write!(f, "I/O error: {}", e),
            ModelError::Network(msg) => write!(f, "Network error: {}", msg),
            ModelError::NoProjectDir => write!(f, "Could not determine platform cache directory"),
            ModelError::AlreadyDownloading => {
                write!(f, "A download for this model is already in progress")
            }
        }
    }
}

impl std::error::Error for ModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ModelError::Io(e) => Some(e),
            _ => None,
        }
    }
}

// =============================================================================
// ModelManager
// =============================================================================

/// Manages Whisper model files: availability checks, path resolution,
/// enumeration, and async download with progress.
///
/// Construct via [`ModelManager::new`]. Does not require a running
/// [`AudioEngine`] instance.
pub struct ModelManager {
    /// Root cache directory: `{platform_cache}/{app_name}/whisper/`
    cache_dir: PathBuf,
    /// Set of model slugs currently being downloaded (guards against concurrent downloads).
    in_progress: Arc<Mutex<HashSet<String>>>,
}

impl ModelManager {
    /// Create a new `ModelManager` for the given application name.
    ///
    /// `app_name` determines the subdirectory inside the platform cache
    /// directory. The whisper model files are stored under
    /// `{platform_cache}/{app_name}/whisper/`.
    ///
    /// # Panics
    ///
    /// Does not panic — if the platform cache directory cannot be determined
    /// the path will fall back to a relative `"."` directory.
    pub fn new(app_name: &str) -> Self {
        let base = directories::BaseDirs::new()
            .map(|d| d.cache_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        let cache_dir = base.join(app_name).join("whisper");

        Self {
            cache_dir,
            in_progress: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Return the expected absolute path for the given model file.
    ///
    /// The file name convention is `ggml-{slug}.bin` where `slug` is the
    /// canonical whisper.cpp model identifier (e.g. `"medium.en"` for
    /// [`WhisperModel::MediumEn`]).
    ///
    /// The file is not required to exist for this method to succeed.
    pub fn path(&self, model: WhisperModel) -> PathBuf {
        self.cache_dir
            .join(format!("ggml-{}.bin", model.slug()))
    }

    /// Return `true` if the model file exists on disk and has a non-zero size.
    pub fn is_available(&self, model: WhisperModel) -> bool {
        let p = self.path(model);
        p.exists()
            && p.metadata()
                .map(|m| m.len() > 0)
                .unwrap_or(false)
    }

    /// Return all cached (available) model variants in ascending order of model
    /// size.
    pub fn list_cached(&self) -> Vec<WhisperModel> {
        WhisperModel::all_in_size_order()
            .iter()
            .copied()
            .filter(|&m| self.is_available(m))
            .collect()
    }

    /// Download a model from Hugging Face with streaming progress.
    ///
    /// `on_progress` is called with a value `0..=100` at start, periodically
    /// during download (at least once per 1 % increment), and at 100 on
    /// completion.
    ///
    /// The download writes to a `.part` temporary file and atomically renames
    /// it to the final path on success, so a partially-downloaded file will
    /// never appear as available.
    ///
    /// Returns `Err(ModelError::AlreadyDownloading)` if a download for the
    /// same model is already in progress on this `ModelManager` instance.
    pub async fn download(
        &self,
        model: WhisperModel,
        on_progress: impl Fn(u8) + Send + 'static,
    ) -> Result<(), ModelError> {
        let slug = model.slug().to_string();

        // Guard against concurrent downloads of the same model.
        {
            let mut in_progress = self
                .in_progress
                .lock()
                .expect("ModelManager in_progress lock poisoned");
            if in_progress.contains(&slug) {
                return Err(ModelError::AlreadyDownloading);
            }
            in_progress.insert(slug.clone());
        }

        let result = self.do_download(model, on_progress).await;

        // Always remove from in-progress set, even on failure.
        {
            let mut in_progress = self
                .in_progress
                .lock()
                .expect("ModelManager in_progress lock poisoned");
            in_progress.remove(&slug);
        }

        result
    }

    /// Internal download implementation.
    async fn do_download(
        &self,
        model: WhisperModel,
        on_progress: impl Fn(u8),
    ) -> Result<(), ModelError> {
        use tokio::io::AsyncWriteExt;

        let dest = self.path(model);

        // Create parent directory if needed.
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(ModelError::Io)?;
        }

        let url = model.download_url();
        tracing::info!("[ModelManager] Downloading {} from {}", model.slug(), url);

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| ModelError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ModelError::Network(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let total_size = response.content_length().unwrap_or(0);
        let mut downloaded: u64 = 0;
        let mut last_percent: u8 = 0;

        on_progress(0);

        // Write to a temporary .part file; rename on success.
        let tmp_path = dest.with_extension("bin.part");
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(ModelError::Io)?;

        let mut stream = response.bytes_stream();
        use futures::StreamExt;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result
                .map_err(|e| ModelError::Network(e.to_string()))?;

            file.write_all(&chunk).await.map_err(ModelError::Io)?;
            downloaded += chunk.len() as u64;

            if total_size > 0 {
                let percent = ((downloaded * 100) / total_size).min(99) as u8;
                if percent > last_percent {
                    on_progress(percent);
                    last_percent = percent;
                }
            }
        }

        file.flush().await.map_err(ModelError::Io)?;
        drop(file);

        tokio::fs::rename(&tmp_path, &dest)
            .await
            .map_err(ModelError::Io)?;

        on_progress(100);
        tracing::info!(
            "[ModelManager] Downloaded {} ({} bytes)",
            model.slug(),
            downloaded
        );

        Ok(())
    }
}
