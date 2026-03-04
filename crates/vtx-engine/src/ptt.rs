//! Push-to-Talk controller.
//!
//! [`PushToTalkController`] accepts application-supplied signals to open and
//! close speech segments. The application is responsible for generating those
//! signals (e.g., from a hotkey, a UI button, an IPC message).
//!
//! The controller is `Clone + Send + 'static` and can be moved to any thread.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::broadcast;
use tracing::info;
use vtx_common::EngineEvent;

use crate::transcription::TranscribeState;

/// Internal PTT activation state, shared between [`PushToTalkController`]
/// clones and the audio loop.
pub struct PttState {
    /// Whether the PTT key is currently held (session open).
    pub is_active: bool,
}

/// Application-agnostic Push-to-Talk controller.
///
/// Obtain via [`AudioEngine::ptt_controller`](crate::AudioEngine::ptt_controller).
///
/// # Example
///
/// ```rust,no_run
/// # async fn example(engine: vtx_engine::AudioEngine) {
/// let ptt = engine.ptt_controller();
/// // In a hotkey callback:
/// ptt.press();
/// // ... record ...
/// ptt.release();
/// # }
/// ```
#[derive(Clone)]
pub struct PushToTalkController {
    state: Arc<Mutex<PttState>>,
    sender: Arc<broadcast::Sender<EngineEvent>>,
    transcribe_state: Arc<Mutex<TranscribeState>>,
    /// Session start time for duration tracking
    session_start: Arc<Mutex<Option<Instant>>>,
}

impl PushToTalkController {
    pub(crate) fn new(
        state: Arc<Mutex<PttState>>,
        sender: Arc<broadcast::Sender<EngineEvent>>,
        transcribe_state: Arc<Mutex<TranscribeState>>,
    ) -> Self {
        Self {
            state,
            sender,
            transcribe_state,
            session_start: Arc::new(Mutex::new(None)),
        }
    }

    /// Signal the start of a PTT session (key-down / button-pressed).
    ///
    /// No-op if a session is already open.
    pub fn press(&self) {
        let already_active = {
            let mut s = self.state.lock().unwrap();
            if s.is_active {
                return;
            }
            s.is_active = true;
            false
        };

        if !already_active {
            info!("[PTT] Session opened");
            *self.session_start.lock().unwrap() = Some(Instant::now());

            // Activate TranscribeState for PTT mode
            if let Ok(mut ts) = self.transcribe_state.lock() {
                ts.set_ptt_mode(true);
                ts.is_active = true;
            }

            let _ = self.sender.send(EngineEvent::SpeechStarted);
        }
    }

    /// Signal the end of a PTT session (key-up / button-released).
    ///
    /// Submits the accumulated audio for transcription and emits
    /// [`EngineEvent::SpeechEnded`]. No-op if no session is open.
    pub fn release(&self) {
        let was_active = {
            let mut s = self.state.lock().unwrap();
            if !s.is_active {
                return;
            }
            s.is_active = false;
            true
        };

        if was_active {
            let duration_ms = self
                .session_start
                .lock()
                .unwrap()
                .take()
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);

            info!("[PTT] Session closed ({}ms)", duration_ms);

            // Submit accumulated audio
            if let Ok(mut ts) = self.transcribe_state.lock() {
                ts.submit_session();
            }

            let _ = self.sender.send(EngineEvent::SpeechEnded { duration_ms });
        }
    }

    /// Convenience: `set_active(true)` == `press()`, `set_active(false)` == `release()`.
    pub fn set_active(&self, active: bool) {
        if active {
            self.press()
        } else {
            self.release()
        }
    }

    /// Whether a PTT session is currently open.
    pub fn is_active(&self) -> bool {
        self.state.lock().map(|s| s.is_active).unwrap_or(false)
    }
}
