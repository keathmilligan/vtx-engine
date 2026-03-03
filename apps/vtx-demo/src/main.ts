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

interface GpuStatus {
  cuda_available: boolean;
  metal_available: boolean;
  system_info: string;
}

interface TranscriptionResult {
  text: string;
  duration_ms: number | null;
  audio_path: string | null;
}

// =============================================================================
// Settings persistence
// =============================================================================

const SETTINGS_KEY = "vtx-demo-settings";

interface AppSettings {
  transcriptionEnabled: boolean;
  autoTranscriptionEnabled: boolean;
  aecEnabled: boolean;
  primaryDeviceId: string;
  secondaryDeviceId: string;
}

function loadSettings(): AppSettings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (raw) {
      return { ...defaultSettings(), ...JSON.parse(raw) };
    }
  } catch {
    // Ignore parse errors — fall through to defaults
  }
  return defaultSettings();
}

function defaultSettings(): AppSettings {
  return {
    transcriptionEnabled: true,
    autoTranscriptionEnabled: false,
    aecEnabled: false,
    primaryDeviceId: "",
    secondaryDeviceId: "",
  };
}

function saveSettings(settings: AppSettings): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Ignore storage errors
  }
}

function getCurrentSettings(): AppSettings {
  return {
    transcriptionEnabled,
    autoTranscriptionEnabled,
    aecEnabled,
    primaryDeviceId: deviceSelect.value,
    secondaryDeviceId: deviceSelect2.value,
  };
}

// =============================================================================
// State
// =============================================================================

let isRecording = false;
let waveformRenderer: WaveformRenderer;
let spectrogramRenderer: SpectrogramRenderer;
let speechActivityRenderer: SpeechActivityRenderer;

// Load persisted settings
const settings = loadSettings();
let transcriptionEnabled = settings.transcriptionEnabled;
let autoTranscriptionEnabled = settings.autoTranscriptionEnabled;
let aecEnabled = settings.aecEnabled;

// =============================================================================
// DOM Elements
// =============================================================================

const statusText = document.getElementById("status-text")!;
const modelStatusEl = document.getElementById("model-status")!;
const gpuStatusEl = document.getElementById("gpu-status")!;
const modelNameEl = document.getElementById("model-name")!;
const deviceSelect = document.getElementById(
  "device-select"
) as HTMLSelectElement;
const deviceSelect2 = document.getElementById(
  "device-select-2"
) as HTMLSelectElement;
const btnCapture = document.getElementById("btn-capture") as HTMLButtonElement;
const btnOpenFile = document.getElementById(
  "btn-open-file"
) as HTMLButtonElement;
const btnDownloadModel = document.getElementById(
  "btn-download-model"
) as HTMLButtonElement;
const transcriptionToggle = document.getElementById(
  "transcription-toggle"
) as HTMLInputElement;
const autoTranscriptionToggle = document.getElementById(
  "auto-transcription-toggle"
) as HTMLInputElement;
const aecToggle = document.getElementById(
  "aec-toggle"
) as HTMLInputElement;
const transcriptionOutput = document.getElementById("transcription-output")!;

// =============================================================================
// Initialization
// =============================================================================

async function init() {
  // Apply persisted toggle states to DOM
  transcriptionToggle.checked = transcriptionEnabled;
  autoTranscriptionToggle.checked = autoTranscriptionEnabled;
  aecToggle.checked = aecEnabled;

  // Set up renderers
  setupRenderers();

  // Set up event listeners
  setupEventListeners();

  // Listen for backend events
  await setupBackendListeners();

  // Wait a moment for engine to initialize, then load devices and sync backend state
  setTimeout(async () => {
    await loadDevices();
    await checkModelStatus();
    await checkGpuStatus();

    // Sync toggle states to the backend after engine is ready
    await syncTogglesToBackend();

    statusText.textContent = "Ready";
  }, 1000);
}

/** Sync current toggle states to the Rust backend after engine init. */
async function syncTogglesToBackend() {
  try {
    await invoke("set_transcription_enabled", { enabled: transcriptionEnabled });
  } catch (e) {
    console.error("Failed to sync transcription enabled:", e);
  }
  try {
    // auto-transcription OFF means PTT mode ON
    await invoke("set_ptt_mode", { enabled: !autoTranscriptionEnabled });
  } catch (e) {
    console.error("Failed to sync PTT mode:", e);
  }
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
  btnCapture.addEventListener("click", toggleRecording);
  btnOpenFile.addEventListener("click", openWavFile);
  btnDownloadModel.addEventListener("click", downloadModel);
  transcriptionToggle.addEventListener("change", onTranscriptionToggle);
  autoTranscriptionToggle.addEventListener("change", onAutoTranscriptionToggle);
  aecToggle.addEventListener("change", onAecToggle);

  // Save device selections on change
  deviceSelect.addEventListener("change", () => saveSettings(getCurrentSettings()));
  deviceSelect2.addEventListener("change", () => saveSettings(getCurrentSettings()));
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
      // Refresh model name display after download
      checkModelStatus();
    } else {
      modelStatusEl.textContent = "Download failed";
    }
  });
}

// =============================================================================
// Device Management
// =============================================================================

function makeOption(device: AudioDevice): HTMLOptionElement {
  const opt = document.createElement("option");
  opt.value = device.id;
  opt.textContent = device.name;
  return opt;
}

function buildDeviceGroups(
  inputDevices: AudioDevice[],
  systemDevices: AudioDevice[],
  includeNone: boolean
): DocumentFragment {
  const frag = document.createDocumentFragment();

  if (includeNone) {
    const noneOpt = document.createElement("option");
    noneOpt.value = "";
    noneOpt.textContent = "None";
    frag.appendChild(noneOpt);
  }

  if (inputDevices.length > 0) {
    const micGroup = document.createElement("optgroup");
    micGroup.label = "Microphone / Input";
    for (const device of inputDevices) {
      micGroup.appendChild(makeOption(device));
    }
    frag.appendChild(micGroup);
  }

  if (systemDevices.length > 0) {
    const sysGroup = document.createElement("optgroup");
    sysGroup.label = "System Audio (Loopback)";
    for (const device of systemDevices) {
      sysGroup.appendChild(makeOption(device));
    }
    frag.appendChild(sysGroup);
  }

  return frag;
}

async function loadDevices() {
  try {
    const [inputDevices, systemDevices] = await Promise.all([
      invoke<AudioDevice[]>("list_input_devices"),
      invoke<AudioDevice[]>("list_system_devices"),
    ]);

    const hasDevices = inputDevices.length > 0 || systemDevices.length > 0;

    if (!hasDevices) {
      for (const sel of [deviceSelect, deviceSelect2]) {
        sel.innerHTML = "";
        const opt = document.createElement("option");
        opt.value = "";
        opt.textContent = "No devices found";
        sel.appendChild(opt);
      }
      return;
    }

    // Primary: no leading None — must pick something
    deviceSelect.innerHTML = "";
    deviceSelect.appendChild(buildDeviceGroups(inputDevices, systemDevices, false));

    // Secondary: same list but with a leading None option
    deviceSelect2.innerHTML = "";
    deviceSelect2.appendChild(buildDeviceGroups(inputDevices, systemDevices, true));

    // Restore saved device selections if they still exist in the list
    const allDeviceIds = [...inputDevices, ...systemDevices].map((d) => d.id);
    if (settings.primaryDeviceId && allDeviceIds.includes(settings.primaryDeviceId)) {
      deviceSelect.value = settings.primaryDeviceId;
    }
    if (settings.secondaryDeviceId === "" || allDeviceIds.includes(settings.secondaryDeviceId)) {
      deviceSelect2.value = settings.secondaryDeviceId;
    }

    btnCapture.disabled = false;
  } catch (e) {
    console.error("Failed to load devices:", e);
    for (const sel of [deviceSelect, deviceSelect2]) {
      sel.innerHTML = "";
      const opt = document.createElement("option");
      opt.value = "";
      opt.textContent = "Failed to load devices";
      sel.appendChild(opt);
    }
  }
}

// =============================================================================
// Recording
// =============================================================================

async function toggleRecording() {
  if (isRecording) {
    await stopRecording();
  } else {
    await startRecording();
  }
}

async function startRecording() {
  const deviceId = deviceSelect.value;
  if (!deviceId) return;

  const source2Id = deviceSelect2.value || null;

  try {
    await invoke("start_capture", {
      sourceId: deviceId,
      source2Id: source2Id,
      aecEnabled: aecEnabled,
    });
    isRecording = true;
    btnCapture.textContent = "Stop";
    btnCapture.classList.add("recording");
    deviceSelect.disabled = true;
    deviceSelect2.disabled = true;
    aecToggle.disabled = true;
    autoTranscriptionToggle.disabled = true;
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
    // In manual mode (auto-transcription OFF), finalize the accumulated segment
    // before stopping capture so the audio is submitted for transcription.
    if (!autoTranscriptionEnabled && transcriptionEnabled) {
      try {
        await invoke("finalize_segment");
      } catch (e) {
        console.error("Failed to finalize segment:", e);
      }
    }

    await invoke("stop_capture");
    isRecording = false;
    btnCapture.textContent = "Record";
    btnCapture.classList.remove("recording");
    deviceSelect.disabled = false;
    deviceSelect2.disabled = false;
    aecToggle.disabled = false;
    autoTranscriptionToggle.disabled = false;
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
      // Extract filename from path (strip directory and extension for display)
      const parts = status.path.replace(/\\/g, "/").split("/");
      const filename = parts[parts.length - 1] ?? status.path;
      const modelName = filename.replace(/\.bin$/, "").replace(/^ggml-/, "");
      modelNameEl.textContent = modelName;
      modelNameEl.title = status.path;
      modelNameEl.className = "status-badge badge-model";
    } else {
      modelStatusEl.textContent = "Model not found";
      btnDownloadModel.style.display = "inline-block";
      modelNameEl.textContent = "";
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

async function checkGpuStatus() {
  try {
    const status = await invoke<GpuStatus>("get_gpu_status");

    if (status.cuda_available) {
      gpuStatusEl.textContent = "CUDA";
      gpuStatusEl.className = "status-badge badge-cuda";
      gpuStatusEl.title = status.system_info;
    } else if (status.metal_available) {
      gpuStatusEl.textContent = "Metal";
      gpuStatusEl.className = "status-badge badge-metal";
      gpuStatusEl.title = status.system_info;
    } else {
      gpuStatusEl.textContent = "CPU";
      gpuStatusEl.className = "status-badge badge-cpu";
      gpuStatusEl.title = status.system_info;
    }
  } catch (e) {
    console.error("Failed to check GPU status:", e);
    gpuStatusEl.textContent = "GPU: unknown";
    gpuStatusEl.className = "status-badge badge-cpu";
  }
}

async function onTranscriptionToggle() {
  transcriptionEnabled = transcriptionToggle.checked;
  saveSettings(getCurrentSettings());
  try {
    await invoke("set_transcription_enabled", { enabled: transcriptionEnabled });
  } catch (e) {
    console.error("Failed to set transcription enabled:", e);
    // Revert the toggle on failure
    transcriptionEnabled = !transcriptionEnabled;
    transcriptionToggle.checked = transcriptionEnabled;
    saveSettings(getCurrentSettings());
  }
}

async function onAutoTranscriptionToggle() {
  autoTranscriptionEnabled = autoTranscriptionToggle.checked;
  saveSettings(getCurrentSettings());
  try {
    // auto-transcription OFF means PTT mode ON (manual submission on stop)
    await invoke("set_ptt_mode", { enabled: !autoTranscriptionEnabled });
  } catch (e) {
    console.error("Failed to set PTT mode:", e);
    // Revert the toggle on failure
    autoTranscriptionEnabled = !autoTranscriptionEnabled;
    autoTranscriptionToggle.checked = autoTranscriptionEnabled;
    saveSettings(getCurrentSettings());
  }
}

function onAecToggle() {
  aecEnabled = aecToggle.checked;
  saveSettings(getCurrentSettings());
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
