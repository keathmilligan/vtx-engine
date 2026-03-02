// vtx-engine Demo Application
//
// Demonstrates live audio capture with visualization, and WAV file transcription.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

// Import visualization renderers
import {
  WaveformRenderer,
  SpectrogramRenderer,
  SpeechActivityRenderer,
} from "@vtx-engine/viz";
import type { VisualizationPayload } from "@vtx-engine/viz";

// =============================================================================
// Types matching Rust backend
// =============================================================================

interface AudioDevice {
  id: string;
  name: string;
  source_type: string;
}

interface ModelStatus {
  available: boolean;
  path: string;
}

interface TranscriptionResult {
  text: string;
  duration_ms: number | null;
  audio_path: string | null;
}

// =============================================================================
// State
// =============================================================================

let isRecording = false;
let waveformRenderer: WaveformRenderer;
let spectrogramRenderer: SpectrogramRenderer;
let speechActivityRenderer: SpeechActivityRenderer;

// =============================================================================
// DOM Elements
// =============================================================================

const statusText = document.getElementById("status-text")!;
const modelStatusEl = document.getElementById("model-status")!;
const deviceSelect = document.getElementById(
  "device-select"
) as HTMLSelectElement;
const btnRecord = document.getElementById("btn-record") as HTMLButtonElement;
const btnStop = document.getElementById("btn-stop") as HTMLButtonElement;
const btnOpenFile = document.getElementById(
  "btn-open-file"
) as HTMLButtonElement;
const btnDownloadModel = document.getElementById(
  "btn-download-model"
) as HTMLButtonElement;
const transcriptionOutput = document.getElementById("transcription-output")!;

// =============================================================================
// Initialization
// =============================================================================

async function init() {
  // Set up renderers
  setupRenderers();

  // Set up event listeners
  setupEventListeners();

  // Listen for backend events
  await setupBackendListeners();

  // Wait a moment for engine to initialize, then load devices
  setTimeout(async () => {
    await loadDevices();
    await checkModelStatus();
    statusText.textContent = "Ready";
  }, 1000);
}

function setupRenderers() {
  const waveformCanvas = document.getElementById(
    "waveform-canvas"
  ) as HTMLCanvasElement;
  const spectrogramCanvas = document.getElementById(
    "spectrogram-canvas"
  ) as HTMLCanvasElement;
  const speechCanvas = document.getElementById(
    "speech-canvas"
  ) as HTMLCanvasElement;

  waveformRenderer = new WaveformRenderer(waveformCanvas);
  spectrogramRenderer = new SpectrogramRenderer(spectrogramCanvas);
  speechActivityRenderer = new SpeechActivityRenderer(speechCanvas);

  // Draw idle state
  waveformRenderer.drawIdle();
  spectrogramRenderer.drawIdle();
  speechActivityRenderer.drawIdle();

  // Handle window resize
  window.addEventListener("resize", () => {
    waveformRenderer.resize();
    spectrogramRenderer.resize();
    speechActivityRenderer.resize();
  });
}

function setupEventListeners() {
  btnRecord.addEventListener("click", startRecording);
  btnStop.addEventListener("click", stopRecording);
  btnOpenFile.addEventListener("click", openWavFile);
  btnDownloadModel.addEventListener("click", downloadModel);
}

async function setupBackendListeners() {
  // Visualization data
  await listen<VisualizationPayload>("visualization-data", (event) => {
    const data = event.payload;

    // Push waveform samples
    if (data.waveform && data.waveform.length > 0) {
      waveformRenderer.pushSamples(data.waveform);
    }

    // Push spectrogram column
    if (data.spectrogram) {
      spectrogramRenderer.pushColumn(data.spectrogram.colors);
    }

    // Push speech metrics
    if (data.speech_metrics) {
      speechActivityRenderer.pushMetrics(data.speech_metrics);
    }
  });

  // Transcription results
  await listen<TranscriptionResult>("transcription-complete", (event) => {
    addTranscriptionResult(event.payload);
  });

  // Capture state changes
  await listen<{ capturing: boolean; error: string | null }>(
    "capture-state-changed",
    (event) => {
      if (event.payload.capturing) {
        statusText.textContent = "Capturing...";
      } else {
        statusText.textContent = "Ready";
      }
      if (event.payload.error) {
        statusText.textContent = `Error: ${event.payload.error}`;
      }
    }
  );

  // Speech events
  await listen("speech-started", () => {
    statusText.textContent = "Speaking...";
  });

  await listen("speech-ended", () => {
    if (isRecording) {
      statusText.textContent = "Capturing...";
    }
  });

  // Model download progress
  await listen<number>("model-download-progress", (event) => {
    modelStatusEl.textContent = `Downloading model: ${event.payload}%`;
  });

  await listen<boolean>("model-download-complete", (event) => {
    if (event.payload) {
      modelStatusEl.textContent = "Model ready";
      btnDownloadModel.style.display = "none";
    } else {
      modelStatusEl.textContent = "Download failed";
    }
  });
}

// =============================================================================
// Device Management
// =============================================================================

async function loadDevices() {
  try {
    const devices = await invoke<AudioDevice[]>("list_input_devices");

    deviceSelect.innerHTML = "";

    if (devices.length === 0) {
      const opt = document.createElement("option");
      opt.value = "";
      opt.textContent = "No input devices found";
      deviceSelect.appendChild(opt);
      return;
    }

    for (const device of devices) {
      const opt = document.createElement("option");
      opt.value = device.id;
      opt.textContent = device.name;
      deviceSelect.appendChild(opt);
    }

    btnRecord.disabled = false;
  } catch (e) {
    console.error("Failed to load devices:", e);
    const opt = document.createElement("option");
    opt.value = "";
    opt.textContent = "Failed to load devices";
    deviceSelect.innerHTML = "";
    deviceSelect.appendChild(opt);
  }
}

// =============================================================================
// Recording
// =============================================================================

async function startRecording() {
  const deviceId = deviceSelect.value;
  if (!deviceId) return;

  try {
    await invoke("start_capture", { sourceId: deviceId });
    isRecording = true;
    btnRecord.disabled = true;
    btnStop.disabled = false;
    deviceSelect.disabled = true;
    statusText.textContent = "Capturing...";

    // Start renderers
    waveformRenderer.start();
    spectrogramRenderer.start();
    speechActivityRenderer.start();
  } catch (e) {
    console.error("Failed to start capture:", e);
    statusText.textContent = `Error: ${e}`;
  }
}

async function stopRecording() {
  try {
    await invoke("stop_capture");
    isRecording = false;
    btnRecord.disabled = false;
    btnStop.disabled = true;
    deviceSelect.disabled = false;
    statusText.textContent = "Ready";

    // Stop renderers
    waveformRenderer.stop();
    spectrogramRenderer.stop();
    speechActivityRenderer.stop();
  } catch (e) {
    console.error("Failed to stop capture:", e);
  }
}

// =============================================================================
// File Transcription
// =============================================================================

async function openWavFile() {
  try {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "WAV Audio",
          extensions: ["wav"],
        },
      ],
    });

    if (!selected) return;

    const filePath = typeof selected === "string" ? selected : selected;
    statusText.textContent = "Transcribing...";
    btnOpenFile.disabled = true;

    try {
      const result = await invoke<TranscriptionResult>("transcribe_file", {
        path: filePath,
      });
      addTranscriptionResult(result);
      statusText.textContent = "Ready";
    } catch (e) {
      console.error("Transcription failed:", e);
      statusText.textContent = `Transcription error: ${e}`;
    } finally {
      btnOpenFile.disabled = false;
    }
  } catch (e) {
    console.error("File dialog error:", e);
  }
}

// =============================================================================
// Model Management
// =============================================================================

async function checkModelStatus() {
  try {
    const status = await invoke<ModelStatus>("check_model_status");
    if (status.available) {
      modelStatusEl.textContent = "Model ready";
      btnDownloadModel.style.display = "none";
    } else {
      modelStatusEl.textContent = "Model not found";
      btnDownloadModel.style.display = "inline-block";
    }
  } catch (e) {
    console.error("Failed to check model status:", e);
  }
}

async function downloadModel() {
  try {
    btnDownloadModel.disabled = true;
    modelStatusEl.textContent = "Starting download...";
    await invoke("download_model");
  } catch (e) {
    console.error("Download failed:", e);
    modelStatusEl.textContent = `Download error: ${e}`;
    btnDownloadModel.disabled = false;
  }
}

// =============================================================================
// Transcription Output
// =============================================================================

function addTranscriptionResult(result: TranscriptionResult) {
  // Remove placeholder
  const placeholder = transcriptionOutput.querySelector(".placeholder");
  if (placeholder) placeholder.remove();

  const div = document.createElement("div");
  div.className = "result";

  const time = document.createElement("span");
  time.className = "time";
  time.textContent = new Date().toLocaleTimeString();

  const text = document.createElement("span");
  text.textContent = result.text;

  div.appendChild(time);
  div.appendChild(text);
  transcriptionOutput.appendChild(div);

  // Scroll to bottom
  transcriptionOutput.scrollTop = transcriptionOutput.scrollHeight;
}

// =============================================================================
// Start
// =============================================================================

init();
