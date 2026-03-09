//! Audio types and processing utilities.

use std::path::PathBuf;

/// Raw recorded audio data before processing.
pub struct RawRecordedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Process raw recorded audio into format suitable for transcription (mono 16kHz).
pub fn process_recorded_audio(raw: RawRecordedAudio) -> Result<Vec<f32>, String> {
    let mono_samples = if raw.channels > 1 {
        convert_to_mono(&raw.samples, raw.channels as usize)
    } else {
        raw.samples
    };
    resample_to_16khz(&mono_samples, raw.sample_rate)
}

/// Get the default recordings directory for temporary WAV files.
pub fn recordings_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vtx-engine")
        .join("recordings")
}

/// Convert multi-channel audio to mono by averaging channels.
pub fn convert_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resample audio to 16kHz using linear interpolation.
pub fn resample_to_16khz(samples: &[f32], source_rate: u32) -> Result<Vec<f32>, String> {
    const TARGET_RATE: u32 = 16000;

    if source_rate == TARGET_RATE {
        return Ok(samples.to_vec());
    }

    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let ratio = source_rate as f64 / TARGET_RATE as f64;
    let output_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_pos = i as f64 * ratio;
        let src_idx = src_pos.floor() as usize;
        let frac = src_pos - src_idx as f64;

        let sample = if src_idx + 1 < samples.len() {
            samples[src_idx] * (1.0 - frac as f32) + samples[src_idx + 1] * frac as f32
        } else if src_idx < samples.len() {
            samples[src_idx]
        } else {
            0.0
        };

        output.push(sample);
    }

    Ok(output)
}

/// Save raw audio samples to a WAV file.
pub fn save_to_wav(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    output_path: &PathBuf,
) -> Result<(), String> {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    let mut writer = WavWriter::create(output_path, spec)
        .map_err(|e| format!("Failed to create WAV file: {}", e))?;

    for &sample in samples {
        writer
            .write_sample(sample)
            .map_err(|e| format!("Failed to write sample: {}", e))?;
    }

    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV file: {}", e))?;

    Ok(())
}

/// Generate a timestamped filename for a VAD segment WAV.
///
/// Includes milliseconds to avoid collisions with the session-level
/// raw/processed WAV pair (which uses second-resolution timestamps via
/// [`generate_recording_stem`]).
pub fn generate_recording_filename() -> String {
    use chrono::Utc;
    let now = Utc::now();
    format!("vtx-{}.wav", now.format("%Y%m%d-%H%M%S%.3f"))
}

/// Generate a timestamped stem (without extension) for recording file pairs.
///
/// Use this when saving multiple related files (e.g., raw + processed) so that
/// both share the same timestamp and differ only by suffix:
/// - `<stem>.wav`           — raw audio
/// - `<stem>-processed.wav` — gain/AGC-processed mono audio
pub fn generate_recording_stem() -> String {
    use chrono::Utc;
    let now = Utc::now();
    format!("vtx-{}", now.format("%Y%m%d-%H%M%S"))
}

/// Extract the recording stem from an existing WAV file path.
///
/// Given a path like `…/vtx-20260308-143022.wav` or
/// `…/vtx-20260308-143022-processed.wav`, returns `Some("vtx-20260308-143022")`.
/// Returns `None` if the filename doesn't follow the expected pattern.
pub fn extract_recording_stem(path: &std::path::Path) -> Option<String> {
    let file_stem = path.file_stem()?.to_str()?;
    // Strip the `-processed` suffix if present to get the base stem.
    let base = file_stem.strip_suffix("-processed").unwrap_or(file_stem);
    // Validate it looks like a vtx recording stem (vtx-YYYYMMDD-HHMMSS).
    if base.starts_with("vtx-") && base.len() >= "vtx-YYYYMMDD-HHMMSS".len() {
        Some(base.to_string())
    } else {
        None
    }
}
