## Requirements

### Requirement: TranscriptionHistory stores a bounded list of transcription entries
`TranscriptionHistory` SHALL be a struct that stores up to `max_entries` `HistoryEntry` values in insertion order. When the store is full and a new entry is added, the oldest entry SHALL be evicted. The store SHALL persist entries to a newline-delimited JSON (NDJSON) file on disk under `{data_dir}/{app_name}/history.ndjson`.

#### Scenario: Entry is appended to the history
- **WHEN** `history.append(entry)` is called
- **THEN** the entry is stored in memory and written to the NDJSON file

#### Scenario: Oldest entry is evicted when capacity is reached
- **WHEN** the history contains `max_entries` entries and `history.append(entry)` is called
- **THEN** the oldest entry is removed and the new entry is added
- **THEN** `history.entries()` returns exactly `max_entries` items

### Requirement: HistoryEntry carries id, text, timestamp, and optional wav_path
`vtx-common` SHALL export `HistoryEntry` with fields: `id: String` (UUID v4), `text: String`, `timestamp: String` (ISO 8601 UTC), and `wav_path: Option<String>`. `HistoryEntry` SHALL derive `Serialize`, `Deserialize`, `Clone`, and `Debug`.

#### Scenario: HistoryEntry serializes to JSON
- **WHEN** a `HistoryEntry` is serialized
- **THEN** the JSON contains `id`, `text`, `timestamp`, and optionally `wav_path` when set

### Requirement: TranscriptionHistory can be opened from the platform-standard data directory
`TranscriptionHistory::open(app_name: &str, max_entries: usize) -> Result<Self, HistoryError>` SHALL resolve the data directory via `directories::ProjectDirs`, create it if absent, and read any existing NDJSON file into memory. If the file does not exist, the history is initialized empty.

#### Scenario: Open creates directory if absent
- **WHEN** `TranscriptionHistory::open("my-app", 200)` is called and the data directory does not exist
- **THEN** the directory is created and an empty history is returned

#### Scenario: Open loads existing entries
- **WHEN** a history file exists with 10 entries and `TranscriptionHistory::open("my-app", 200)` is called
- **THEN** the returned history contains those 10 entries

### Requirement: WAV files associated with entries can be retained and cleaned up
`TranscriptionHistory` SHALL expose `cleanup_wav_files(ttl: std::time::Duration)` which removes WAV files referenced by entries whose `timestamp` is older than the TTL. The corresponding `wav_path` field on evicted entries SHALL be set to `None` and the history file SHALL be rewritten.

#### Scenario: Old WAV files are deleted
- **WHEN** `history.cleanup_wav_files(Duration::from_secs(86400))` is called and an entry's timestamp is more than 24 hours ago
- **THEN** the WAV file at `wav_path` is deleted from disk
- **THEN** the entry's `wav_path` field is set to `None` in the history file

#### Scenario: Recent WAV files are not deleted
- **WHEN** `history.cleanup_wav_files(Duration::from_secs(86400))` is called and an entry's timestamp is less than 24 hours ago
- **THEN** the WAV file is not deleted

### Requirement: TranscriptionHistoryRecorder auto-appends from the broadcast channel
The library SHALL provide `TranscriptionHistoryRecorder`, a type constructed from a `broadcast::Receiver<EngineEvent>` and a `TranscriptionHistory`. Calling `recorder.start()` SHALL spawn a task that listens for `TranscriptionComplete` events and appends a `HistoryEntry` for each one. The recorder SHALL stop when the broadcast channel is closed.

#### Scenario: Recorder appends transcription results
- **WHEN** a `TranscriptionHistoryRecorder` is started and a `TranscriptionComplete` event is emitted
- **THEN** the history gains a new entry with the transcription text and a generated UUID and timestamp

#### Scenario: Recorder stops cleanly when channel closes
- **WHEN** the `AudioEngine` is dropped and the broadcast sender is closed
- **THEN** the recorder task exits without panicking

### Requirement: Individual history entries can be deleted by id
`TranscriptionHistory` SHALL expose `delete(id: &str) -> bool` which removes the entry with the given `id`. The WAV file referenced by `wav_path` (if any) SHALL be deleted from disk. The history file SHALL be rewritten. Returns `true` if an entry was found and deleted, `false` otherwise.

#### Scenario: Delete removes entry and WAV file
- **WHEN** `history.delete("some-uuid")` is called for an existing entry with a WAV file
- **THEN** the entry is removed from memory and the NDJSON file
- **THEN** the WAV file is deleted from disk
- **THEN** the method returns `true`

#### Scenario: Delete returns false for unknown id
- **WHEN** `history.delete("nonexistent-id")` is called
- **THEN** no entries are removed and `false` is returned
