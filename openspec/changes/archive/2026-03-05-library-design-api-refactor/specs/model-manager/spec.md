## ADDED Requirements

### Requirement: ModelManager is a standalone public struct
The library SHALL expose a `ModelManager` struct in `vtx-engine`. `ModelManager::new(app_name: &str) -> ModelManager` SHALL be the only constructor. The `app_name` parameter determines the cache directory root (`{cache_dir}/{app_name}/whisper/`). `ModelManager` SHALL NOT require a running `AudioEngine` instance.

#### Scenario: Construction succeeds for a valid app name
- **WHEN** `ModelManager::new("my-app")` is called
- **THEN** a `ModelManager` is returned without error

### Requirement: ModelManager resolves canonical model file paths
`ModelManager::path(model: WhisperModel) -> PathBuf` SHALL return the expected absolute path for the given model file even if the file does not yet exist. The filename convention SHALL be `ggml-{slug}.bin` where `slug` is the canonical whisper.cpp model identifier (e.g. `WhisperModel::MediumEn` → `ggml-medium.en.bin`).

#### Scenario: Path for MediumEn resolves correctly
- **WHEN** `manager.path(WhisperModel::MediumEn)` is called
- **THEN** the returned path ends with `whisper/ggml-medium.en.bin` under the platform cache directory for the given `app_name`

### Requirement: ModelManager reports model availability
`ModelManager::is_available(model: WhisperModel) -> bool` SHALL return `true` if and only if the model file returned by `ModelManager::path(model)` exists on disk and has a non-zero file size.

#### Scenario: Returns true when model file exists
- **WHEN** the model file is present on disk
- **THEN** `manager.is_available(model)` returns `true`

#### Scenario: Returns false when model file is absent
- **WHEN** no model file exists at the expected path
- **THEN** `manager.is_available(model)` returns `false`

### Requirement: ModelManager enumerates cached models
`ModelManager::list_cached() -> Vec<WhisperModel>` SHALL return a list of all `WhisperModel` variants for which `is_available()` would return `true`, in ascending order of model size.

#### Scenario: Lists only downloaded models
- **WHEN** only `WhisperModel::BaseEn` and `WhisperModel::MediumEn` files are present in the cache directory
- **THEN** `manager.list_cached()` returns `[WhisperModel::BaseEn, WhisperModel::MediumEn]` (or in size order)

### Requirement: ModelManager downloads models asynchronously with progress
`ModelManager::download(model: WhisperModel, on_progress: impl Fn(u8) + Send + 'static) -> async Result<(), ModelError>` SHALL download the model file from the canonical Hugging Face URL for the given variant. It SHALL call `on_progress` with a value `0..=100` at least on start and on completion. The download SHALL write to a `.download` temp file and atomically rename to the final path on success. If a download for the same model is already in progress, the method SHALL return `Err(ModelError::AlreadyDownloading)`.

#### Scenario: Successful download completes and file is available
- **WHEN** `manager.download(WhisperModel::TinyEn, |_| {}).await` is called and the network request succeeds
- **THEN** the method returns `Ok(())`
- **THEN** `manager.is_available(WhisperModel::TinyEn)` returns `true`

#### Scenario: Progress callback is called with 100 on completion
- **WHEN** `manager.download(WhisperModel::TinyEn, |pct| capture(pct)).await` succeeds
- **THEN** the callback was called with `100` as the final invocation

#### Scenario: Concurrent download for same model returns error
- **WHEN** `manager.download(WhisperModel::TinyEn, |_| {})` is called while a download for `TinyEn` is already in progress
- **THEN** the second call returns `Err(ModelError::AlreadyDownloading)` immediately

### Requirement: ModelError provides typed failure cases
`ModelError` SHALL be a public enum in `vtx-engine` with at least the variants: `Io(std::io::Error)`, `Network(String)`, `NoProjectDir`, and `AlreadyDownloading`.

#### Scenario: Network failure surfaces as ModelError::Network
- **WHEN** the Hugging Face download URL is unreachable
- **THEN** `manager.download(...)` returns `Err(ModelError::Network(_))`

### Requirement: WhisperModel enum covers all supported variants
`WhisperModel` SHALL be a public enum in `vtx-common` with variants: `TinyEn`, `Tiny`, `BaseEn`, `Base`, `SmallEn`, `Small`, `MediumEn`, `Medium`, `LargeV3`. It SHALL derive `serde::Serialize`, `serde::Deserialize`, `Clone`, `Copy`, `PartialEq`, `Eq`, and `Debug`. Serialization SHALL use snake_case variant names (`"tiny_en"`, `"base_en"`, etc.).

#### Scenario: WhisperModel round-trips through JSON
- **WHEN** `WhisperModel::MediumEn` is serialized to JSON and deserialized
- **THEN** the deserialized value equals `WhisperModel::MediumEn`
