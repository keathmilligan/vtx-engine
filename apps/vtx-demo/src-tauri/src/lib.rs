//! vtx-demo: Demo application for the vtx-engine voice processing library.
//!
//! Provides a simple UI to test live audio capture, visualization, and
//! WAV file transcription.

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;
use tracing::info;

use vtx_common::*;
use vtx_engine::{AudioEngine, EngineConfig, EventHandler};

/// Application state shared across Tauri commands.
struct AppState {
    engine: Arc<Mutex<Option<AudioEngine>>>,
}

/// Event handler that forwards engine events to the Tauri frontend.
struct TauriEventHandler {
    app_handle: tauri::AppHandle,
}

impl EventHandler for TauriEventHandler {
    fn on_event(&self, event: EngineEvent) {
        match &event {
            EngineEvent::VisualizationData(data) => {
                let _ = self.app_handle.emit("visualization-data", data);
            }
            EngineEvent::TranscriptionComplete(result) => {
                let _ = self.app_handle.emit("transcription-complete", result);
            }
            EngineEvent::SpeechStarted => {
                let _ = self.app_handle.emit("speech-started", ());
            }
            EngineEvent::SpeechEnded { duration_ms } => {
                let _ = self.app_handle.emit("speech-ended", duration_ms);
            }
            EngineEvent::CaptureStateChanged { capturing, error } => {
                #[derive(serde::Serialize, Clone)]
                struct CaptureState {
                    capturing: bool,
                    error: Option<String>,
                }
                let _ = self.app_handle.emit(
                    "capture-state-changed",
                    CaptureState {
                        capturing: *capturing,
                        error: error.clone(),
                    },
                );
            }
            EngineEvent::ModelDownloadProgress { percent } => {
                let _ = self.app_handle.emit("model-download-progress", percent);
            }
            EngineEvent::ModelDownloadComplete { success } => {
                let _ = self.app_handle.emit("model-download-complete", success);
            }
            EngineEvent::AudioLevelUpdate { device_id, level_db } => {
                #[derive(serde::Serialize, Clone)]
                struct LevelUpdate {
                    device_id: String,
                    level_db: f32,
                }
                let _ = self.app_handle.emit(
                    "audio-level-update",
                    LevelUpdate {
                        device_id: device_id.clone(),
                        level_db: *level_db,
                    },
                );
            }
        }
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
async fn start_capture(
    state: tauri::State<'_, AppState>,
    source_id: String,
) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.start_capture(Some(source_id), None).await
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
) -> Result<TranscriptionResult, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.transcribe_file(path).await
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<EngineStatus, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.get_status())
}

#[tauri::command]
async fn get_gpu_status(state: tauri::State<'_, AppState>) -> Result<vtx_common::GpuStatus, String> {
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

// =============================================================================
// App Entry Point
// =============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
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

            // Initialize engine in background
            let state = AppState {
                engine: Arc::new(Mutex::new(None)),
            };

            let engine_arc = state.engine.clone();

            // Spawn engine initialization
            tauri::async_runtime::spawn(async move {
                let handler = TauriEventHandler {
                    app_handle: app_handle.clone(),
                };

                match AudioEngine::new(EngineConfig::default(), handler).await {
                    Ok(engine) => {
                        info!("Audio engine initialized successfully");
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running vtx-demo");
}
