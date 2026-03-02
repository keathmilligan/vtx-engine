//! Voice transcription module.
//!
//! Provides automatic transcription of audio using whisper.cpp via FFI.
//!
//! # Components
//!
//! - [`whisper_ffi`]: Low-level FFI bindings to whisper.cpp
//! - [`transcriber`]: High-level transcription API
//! - [`queue`]: Transcription queue with worker thread
//! - [`transcribe_state`]: State management for continuous transcription

pub mod queue;
pub mod transcribe_state;
pub mod transcriber;
pub mod whisper_ffi;

pub use queue::{TranscriptionCallback, TranscriptionQueue};
pub use transcribe_state::TranscribeState;
pub use transcriber::{download_model, Transcriber};
