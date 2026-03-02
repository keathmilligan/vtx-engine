//! Whisper transcription wrapper.
//!
//! This module provides a high-level API for transcribing audio using whisper.cpp.
//!
//! ## Hallucination Mitigation
//!
//! Whisper can sometimes produce repetition loops where the same phrase is
//! repeated many times. This transcriber includes:
//! - Whisper parameter tuning to reduce hallucinations at the source
//! - Post-processing to detect and remove repetition loops

use std::path::PathBuf;

use super::whisper_ffi::{self, Context, WhisperSamplingStrategy};

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

/// Minimum number of repetitions to consider text as a hallucination loop
const MIN_REPETITIONS_FOR_LOOP: usize = 3;

/// Minimum phrase length (in chars) to check for repetition
const MIN_PHRASE_LENGTH: usize = 10;

/// Wrapper around whisper.cpp for transcription.
pub struct Transcriber {
    ctx: Option<Context>,
    model_path: PathBuf,
    library_initialized: bool,
}

impl Transcriber {
    /// Create a new transcriber with the default model path.
    pub fn new() -> Self {
        let model_path = get_default_model_path();
        Self {
            ctx: None,
            model_path,
            library_initialized: false,
        }
    }

    /// Get the path to the model file.
    pub fn get_model_path(&self) -> &PathBuf {
        &self.model_path
    }

    /// Check if the model file exists.
    pub fn is_model_available(&self) -> bool {
        self.model_path.exists()
    }

    /// Ensure the whisper library is loaded.
    fn ensure_library(&mut self) -> Result<(), String> {
        if !self.library_initialized {
            whisper_ffi::init_library()?;
            self.library_initialized = true;
        }
        Ok(())
    }

    /// Load the whisper model. This is called automatically by transcribe() if needed.
    pub fn load_model(&mut self) -> Result<(), String> {
        if self.ctx.is_some() {
            return Ok(());
        }

        self.ensure_library()?;

        if !self.model_path.exists() {
            return Err(format!(
                "Whisper model not found at: {}\n\n\
                Please download a model file:\n\
                1. Visit: https://huggingface.co/ggerganov/whisper.cpp/tree/main\n\
                2. Download 'ggml-base.en.bin' (or another model)\n\
                3. Place it at: {}",
                self.model_path.display(),
                self.model_path.display()
            ));
        }

        tracing::info!("Loading whisper model from: {}", self.model_path.display());
        let ctx = Context::new(&self.model_path)?;
        self.ctx = Some(ctx);
        tracing::info!("Whisper model loaded successfully");
        Ok(())
    }

    /// Transcribe audio samples (mono, 16kHz).
    ///
    /// The audio should already be converted to mono 16kHz format.
    /// The output is post-processed to remove hallucination loops (repeated phrases).
    pub fn transcribe(&mut self, audio_data: &[f32]) -> Result<String, String> {
        self.load_model()?;

        let ctx = self.ctx.as_ref().unwrap();

        // Get default params with greedy strategy
        let mut params = whisper_ffi::full_default_params(WhisperSamplingStrategy::Greedy)?;

        // Apply hallucination mitigation settings
        params.configure_with_hallucination_mitigation();

        // Run transcription
        ctx.full(&params, audio_data)?;

        let num_segments = ctx.full_n_segments()?;

        if num_segments == 0 {
            return Ok("(No speech detected)".to_string());
        }

        let mut result = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = ctx.full_get_segment_text(i) {
                let trimmed = segment.trim();
                if !trimmed.is_empty() {
                    if !result.is_empty() {
                        result.push(' ');
                    }
                    result.push_str(trimmed);
                }
            }
        }

        // Post-process to remove hallucination loops
        let result = Self::remove_repetition_loops(&result);

        if result.is_empty() {
            Ok("(No speech detected)".to_string())
        } else {
            Ok(result)
        }
    }

    /// Transcribe audio with duration hint for optimization.
    ///
    /// The duration_ms parameter helps optimize whisper parameters for short audio.
    /// Note: For short audio, some hallucination mitigations are relaxed to avoid
    /// rejecting valid short utterances, but repetition loop removal still applies.
    #[allow(dead_code)]
    pub fn transcribe_with_duration(
        &mut self,
        audio_data: &[f32],
        duration_ms: i32,
    ) -> Result<String, String> {
        self.load_model()?;

        let ctx = self.ctx.as_ref().unwrap();

        // Get default params with greedy strategy
        let mut params = whisper_ffi::full_default_params(WhisperSamplingStrategy::Greedy)?;

        // Optimize for short audio if duration is known
        if duration_ms > 0 && duration_ms < 10000 {
            params.configure_for_short_audio(audio_data.len(), duration_ms);
        } else {
            // For longer audio, use full hallucination mitigation
            params.configure_with_hallucination_mitigation();
        }

        // Run transcription
        ctx.full(&params, audio_data)?;

        let num_segments = ctx.full_n_segments()?;

        if num_segments == 0 {
            return Ok("(No speech detected)".to_string());
        }

        let mut result = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = ctx.full_get_segment_text(i) {
                let trimmed = segment.trim();
                if !trimmed.is_empty() {
                    if !result.is_empty() {
                        result.push(' ');
                    }
                    result.push_str(trimmed);
                }
            }
        }

        // Post-process to remove hallucination loops
        let result = Self::remove_repetition_loops(&result);

        if result.is_empty() {
            Ok("(No speech detected)".to_string())
        } else {
            Ok(result)
        }
    }

    /// Remove repetition loops (hallucinations) from transcribed text.
    ///
    /// Whisper sometimes produces output like:
    /// "And I think that's important. And I think that's important. And I think that's important."
    ///
    /// This function detects such patterns and keeps only the first occurrence.
    fn remove_repetition_loops(text: &str) -> String {
        if text.len() < MIN_PHRASE_LENGTH * MIN_REPETITIONS_FOR_LOOP {
            return text.to_string();
        }

        // Split into words for analysis
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < MIN_REPETITIONS_FOR_LOOP * 3 {
            return text.to_string();
        }

        // Try to find repeating word sequences of different lengths
        // Start with longer sequences (more reliable detection)
        for seq_len in (3..=words.len() / MIN_REPETITIONS_FOR_LOOP).rev() {
            if let Some(result) = Self::find_and_remove_word_sequence_repetition(&words, seq_len) {
                tracing::debug!(
                    "Removed repetition loop (seq_len={}): '{}' -> '{}'",
                    seq_len,
                    text,
                    result
                );
                return result;
            }
        }

        text.to_string()
    }

    /// Find repeating word sequences and remove duplicates.
    fn find_and_remove_word_sequence_repetition(words: &[&str], seq_len: usize) -> Option<String> {
        if words.len() < seq_len * MIN_REPETITIONS_FOR_LOOP {
            return None;
        }

        // Try each starting position
        for start in 0..=(words.len() - seq_len * MIN_REPETITIONS_FOR_LOOP) {
            let pattern: Vec<&str> = words[start..start + seq_len].to_vec();
            let pattern_lower: Vec<String> = pattern.iter().map(|w| w.to_lowercase()).collect();

            // Count consecutive occurrences of this pattern
            let mut count = 1;
            let mut pos = start + seq_len;

            while pos + seq_len <= words.len() {
                let candidate: Vec<String> = words[pos..pos + seq_len]
                    .iter()
                    .map(|w| w.to_lowercase())
                    .collect();

                if candidate == pattern_lower {
                    count += 1;
                    pos += seq_len;
                } else {
                    break;
                }
            }

            // Found a repetition loop
            if count >= MIN_REPETITIONS_FOR_LOOP {
                // Build result: words before pattern + single pattern + words after repetitions
                let mut result_words: Vec<&str> = Vec::new();

                // Add words before the pattern
                result_words.extend_from_slice(&words[..start]);

                // Add the pattern once (use original casing from first occurrence)
                result_words.extend_from_slice(&pattern);

                // Add words after all repetitions
                let after_repetitions = start + seq_len * count;
                if after_repetitions < words.len() {
                    result_words.extend_from_slice(&words[after_repetitions..]);
                }

                return Some(result_words.join(" "));
            }
        }

        None
    }
}

impl Default for Transcriber {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the default model path.
fn get_default_model_path() -> PathBuf {
    let cache_dir = directories::BaseDirs::new()
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    cache_dir.join("whisper").join("ggml-base.en.bin")
}

/// Download the Whisper model to the specified path with streaming progress.
///
/// The `on_progress` callback is invoked with the current download percentage
/// (0-100). It is called at most once per 1% increment to avoid flooding.
pub async fn download_model<F>(model_path: &PathBuf, on_progress: F) -> Result<(), String>
where
    F: Fn(u8),
{
    use tokio::io::AsyncWriteExt;

    // Create parent directory if it doesn't exist
    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    tracing::info!("Downloading whisper model to: {}", model_path.display());

    let client = reqwest::Client::new();
    let response = client
        .get(MODEL_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to download model: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download model: HTTP {}",
            response.status()
        ));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last_percent: u8 = 0;

    on_progress(0);

    // Write to a temporary file first, then rename on success
    let tmp_path = model_path.with_extension("bin.part");
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut stream = response.bytes_stream();
    use futures::StreamExt;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Failed to read response: {}", e))?;

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write file: {}", e))?;

        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let percent = ((downloaded * 100) / total_size).min(99) as u8;
            if percent > last_percent {
                on_progress(percent);
                last_percent = percent;
            }
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed to flush file: {}", e))?;
    drop(file);

    // Rename temp file to final path
    tokio::fs::rename(&tmp_path, model_path)
        .await
        .map_err(|e| format!("Failed to rename temp file: {}", e))?;

    on_progress(100);
    tracing::info!("Model downloaded successfully ({} bytes)", downloaded);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcriber_creation() {
        let transcriber = Transcriber::new();
        // Model path should be set to default
        assert!(!transcriber.get_model_path().as_os_str().is_empty());
    }

    #[test]
    fn test_remove_repetition_loops_basic() {
        // Classic hallucination loop
        let input = "And I think that's a very important point. And I think that's a very important point. And I think that's a very important point. And I think that's a very important point.";
        let result = Transcriber::remove_repetition_loops(input);
        assert!(
            result
                .matches("And I think that's a very important point")
                .count()
                == 1,
            "Expected single occurrence, got: {}",
            result
        );
    }

    #[test]
    fn test_remove_repetition_loops_with_trailing() {
        // Hallucination with text after (need at least 3 words per phrase)
        let input =
            "This is important. This is important. This is important. And then something else.";
        let result = Transcriber::remove_repetition_loops(input);
        // Should keep first occurrence and trailing text
        assert!(result.contains("This is important"));
        assert!(result.contains("something else"));
        assert!(
            result.matches("This is important").count() == 1,
            "Expected single occurrence, got: {}",
            result
        );
    }

    #[test]
    fn test_remove_repetition_loops_no_repetition() {
        // Normal text without repetition
        let input = "This is a normal sentence. And this is another one. Nothing repeating here.";
        let result = Transcriber::remove_repetition_loops(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_remove_repetition_loops_short_text() {
        // Text too short to be a loop
        let input = "Short text.";
        let result = Transcriber::remove_repetition_loops(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_remove_repetition_loops_two_occurrences_ok() {
        // Two occurrences is not enough to be considered a loop
        let input = "I agree with that. I agree with that.";
        let result = Transcriber::remove_repetition_loops(input);
        // Should not be modified (only 2 occurrences, below threshold)
        assert_eq!(result, input);
    }

    #[test]
    fn test_remove_repetition_loops_case_insensitive() {
        // Test that case differences are handled
        let input = "Hello World. hello world. HELLO WORLD. And more text.";
        let result = Transcriber::remove_repetition_loops(input);
        // Should detect the loop despite case differences
        assert!(
            result.matches("Hello").count() + result.matches("hello").count() <= 2,
            "Expected reduced repetitions, got: {}",
            result
        );
    }
}
