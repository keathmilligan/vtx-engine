//! Transcription history store.
//!
//! Entries are persisted as newline-delimited JSON (NDJSON) at:
//! `{data_dir}/{app_name}/history.ndjson`
//!
//! WAV files referenced by entries are stored under:
//! `{data_dir}/{app_name}/recordings/`

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::{EngineEvent, HistoryEntry};
use chrono::Utc;
use directories::ProjectDirs;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

const HISTORY_FILENAME: &str = "history.ndjson";

// =============================================================================
// HistoryError
// =============================================================================

/// Errors from history store operations.
#[derive(Debug)]
pub enum HistoryError {
    /// I/O error.
    Io(std::io::Error),
    /// JSON parse error.
    Parse(String),
    /// Platform data directory could not be determined.
    NoProjectDir,
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HistoryError::Io(e) => write!(f, "I/O error: {}", e),
            HistoryError::Parse(s) => write!(f, "Parse error: {}", s),
            HistoryError::NoProjectDir => write!(f, "Cannot determine data directory"),
        }
    }
}

impl std::error::Error for HistoryError {}

// =============================================================================
// TranscriptionHistory
// =============================================================================

/// Bounded transcription history store backed by NDJSON on disk.
pub struct TranscriptionHistory {
    entries: VecDeque<HistoryEntry>,
    max_entries: usize,
    history_path: std::path::PathBuf,
}

impl TranscriptionHistory {
    /// Open (or create) the history store for the given application.
    ///
    /// Data is stored at `{data_dir}/{app_name}/history.ndjson`.
    pub fn open(app_name: &str, max_entries: usize) -> Result<Self, HistoryError> {
        let data_dir = resolve_data_dir(app_name)?;
        std::fs::create_dir_all(&data_dir).map_err(HistoryError::Io)?;

        let history_path = data_dir.join(HISTORY_FILENAME);

        let mut entries = VecDeque::with_capacity(max_entries);

        if history_path.exists() {
            let content = std::fs::read_to_string(&history_path).map_err(HistoryError::Io)?;
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<HistoryEntry>(line) {
                    Ok(entry) => entries.push_back(entry),
                    Err(e) => warn!("[History] Skipping malformed entry: {}", e),
                }
            }
        }

        Ok(Self {
            entries,
            max_entries,
            history_path,
        })
    }

    /// Append a new entry. If capacity is reached, the oldest entry is evicted.
    pub fn append(&mut self, entry: HistoryEntry) {
        let evicted = if self.entries.len() >= self.max_entries {
            self.entries.pop_front()
        } else {
            None
        };

        self.entries.push_back(entry);

        if evicted.is_some() {
            // Full rewrite needed (eviction changes the file)
            let _ = self.rewrite();
        } else {
            // Append only the new entry
            let _ = self.append_to_file(self.entries.back().unwrap());
        }
    }

    /// Return all history entries in insertion order.
    pub fn entries(&self) -> &[HistoryEntry] {
        self.entries.as_slices().0
    }

    /// Delete the entry with the given id.
    ///
    /// Deletes the associated WAV file (if any) and rewrites the history file.
    /// Returns `true` if an entry was found and removed.
    pub fn delete(&mut self, id: &str) -> bool {
        let pos = self.entries.iter().position(|e| e.id == id);
        if let Some(idx) = pos {
            let entry = self.entries.remove(idx).unwrap();
            if let Some(ref wav) = entry.wav_path {
                let _ = std::fs::remove_file(wav);
            }
            let _ = self.rewrite();
            true
        } else {
            false
        }
    }

    /// Delete WAV files for entries older than `ttl`. Clears `wav_path` on
    /// affected entries and rewrites the history file.
    pub fn cleanup_wav_files(&mut self, ttl: std::time::Duration) {
        let now = Utc::now();
        let mut changed = false;

        for entry in self.entries.iter_mut() {
            if let Some(ref wav) = entry.wav_path.clone() {
                if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&entry.timestamp) {
                    let age = now.signed_duration_since(ts.with_timezone(&Utc));
                    if age > chrono::Duration::from_std(ttl).unwrap_or_default() {
                        let _ = std::fs::remove_file(wav);
                        entry.wav_path = None;
                        changed = true;
                    }
                }
            }
        }

        if changed {
            let _ = self.rewrite();
        }
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    fn append_to_file(&self, entry: &HistoryEntry) -> Result<(), HistoryError> {
        use std::io::Write;
        let json = serde_json::to_string(entry).map_err(|e| HistoryError::Parse(e.to_string()))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_path)
            .map_err(HistoryError::Io)?;
        writeln!(file, "{}", json).map_err(HistoryError::Io)
    }

    fn rewrite(&self) -> Result<(), HistoryError> {
        use std::io::Write;
        let mut file = std::fs::File::create(&self.history_path).map_err(HistoryError::Io)?;
        for entry in &self.entries {
            let json =
                serde_json::to_string(entry).map_err(|e| HistoryError::Parse(e.to_string()))?;
            writeln!(file, "{}", json).map_err(HistoryError::Io)?;
        }
        Ok(())
    }
}

// =============================================================================
// TranscriptionHistoryRecorder
// =============================================================================

/// Subscribes to the engine broadcast channel and appends a [`HistoryEntry`]
/// for every [`EngineEvent::TranscriptionComplete`] event.
pub struct TranscriptionHistoryRecorder {
    receiver: broadcast::Receiver<EngineEvent>,
    history: Arc<Mutex<TranscriptionHistory>>,
}

impl TranscriptionHistoryRecorder {
    /// Create a recorder that writes to the given shared history store.
    pub fn new(
        receiver: broadcast::Receiver<EngineEvent>,
        history: Arc<Mutex<TranscriptionHistory>>,
    ) -> Self {
        Self { receiver, history }
    }

    /// Spawn a tokio task that listens for events and appends history entries.
    /// The task exits cleanly when the broadcast channel closes.
    pub fn start(mut self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                match self.receiver.recv().await {
                    Ok(EngineEvent::TranscriptionComplete(result)) => {
                        let entry = HistoryEntry {
                            id: Uuid::new_v4().to_string(),
                            text: result.text.clone(),
                            timestamp: Utc::now().to_rfc3339(),
                            wav_path: result.audio_path.clone(),
                        };
                        info!("[History] Recording entry: {}", entry.id);
                        if let Ok(mut h) = self.history.lock() {
                            h.append(entry);
                        }
                    }
                    Ok(_) => {} // Ignore other events
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("[History] Lagged: dropped {} events", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn resolve_data_dir(app_name: &str) -> Result<std::path::PathBuf, HistoryError> {
    if app_name.is_empty() {
        return Err(HistoryError::NoProjectDir);
    }
    let dirs = ProjectDirs::from("", "", app_name).ok_or(HistoryError::NoProjectDir)?;
    Ok(dirs.data_dir().to_path_buf())
}
