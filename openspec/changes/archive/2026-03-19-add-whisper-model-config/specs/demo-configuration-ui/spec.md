## ADDED Requirements

### Requirement: Configuration panel includes Model section as first section
The configuration panel SHALL include a Model section positioned before the Audio Input section. The Model section SHALL display all available `WhisperModel` variants with their download status and allow model selection and downloading.

#### Scenario: Model section appears before Audio Input
- **WHEN** the configuration panel is opened
- **THEN** the Model section is visible as the first section
- **THEN** the Audio Input section appears after the Model section

#### Scenario: Model section fetches status on panel open
- **WHEN** the configuration panel is opened
- **THEN** `get_model_status` is invoked to populate the model list
- **THEN** the current `EngineConfig.model` is shown as selected
