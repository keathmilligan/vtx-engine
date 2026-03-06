//! vtx-demo: Demo application for the vtx-engine voice processing library.
//!
//! Provides a simple UI to test live audio capture, visualization, and
//! WAV file transcription.

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;
use tracing::info;

use vtx_engine::{AudioEngine, EngineBuilder};
use vtx_engine::*;

/// Application state shared across Tauri commands.
struct AppState {
    engine: Arc<Mutex<Option<AudioEngine>>>,
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
async fn list_system_devices(state: tauri::State<'_, AppState>) -> Result<Vec<AudioDevice>, String> {
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
async fn stop_recording(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    engine.stop_recording();
    Ok(())
}

#[tauri::command]
async fn is_recording(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let engine_lock = state.engine.lock().await;
    let engine = engine_lock.as_ref().ok_or("Engine not initialized")?;
    Ok(engine.is_recording())
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
            };

            let engine_arc = state.engine.clone();

            tauri::async_runtime::spawn(async move {
                match EngineBuilder::new().build().await {
                    Ok((engine, rx)) => {
                        info!("Audio engine initialized successfully");

                        // Spawn event forwarding task using EventHandlerAdapter pattern
                        let ah = app_handle.clone();
                        vtx_engine::EventHandlerAdapter::new(rx, move |event| {
                            match &event {
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
                                    struct CaptureState { capturing: bool, error: Option<String> }
                                    let _ = ah.emit("capture-state-changed", CaptureState {
                                        capturing: *capturing,
                                        error: error.clone(),
                                    });
                                }
                                EngineEvent::ModelDownloadProgress { percent } => {
                                    let _ = ah.emit("model-download-progress", percent);
                                }
                                EngineEvent::ModelDownloadComplete { success } => {
                                    let _ = ah.emit("model-download-complete", success);
                                }
                                EngineEvent::AudioLevelUpdate { device_id, level_db } => {
                                    #[derive(serde::Serialize, Clone)]
                                    struct LevelUpdate { device_id: String, level_db: f32 }
                                    let _ = ah.emit("audio-level-update", LevelUpdate {
                                        device_id: device_id.clone(),
                                        level_db: *level_db,
                                    });
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
                            }
                        }).spawn();

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running vtx-demo");
}
