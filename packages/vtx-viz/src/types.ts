// Types matching the vtx-common Rust types (serialized via serde)

/** A single column of spectrogram data from the backend. */
export interface SpectrogramColumn {
  /** RGB triplets for each pixel row. */
  colors: number[];
}

/** Speech detection metrics from the backend. */
export interface SpeechMetrics {
  /** RMS amplitude in decibels. */
  amplitude_db: number;
  /** Zero-crossing rate (0.0 to 0.5). */
  zcr: number;
  /** Estimated spectral centroid in Hz. */
  centroid_hz: number;
  /** Whether speech is currently detected. */
  is_speaking: boolean;
  /** Whether voiced speech onset is pending. */
  voiced_onset_pending: boolean;
  /** Whether whisper speech onset is pending. */
  whisper_onset_pending: boolean;
  /** Whether current frame is classified as transient. */
  is_transient: boolean;
  /** Whether this is lookback-determined speech. */
  is_lookback_speech: boolean;
  /** Whether a word break (inter-word gap) is detected. */
  is_word_break: boolean;
}

/** Visualization data payload from the engine. */
export interface VisualizationPayload {
  /** Pre-downsampled waveform amplitudes. */
  waveform: number[];
  /** Spectrogram column (present when FFT buffer fills). */
  spectrogram: SpectrogramColumn | null;
  /** Speech detection metrics (present when speech processor is active). */
  speech_metrics: SpeechMetrics | null;
}
