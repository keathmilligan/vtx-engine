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

interface ModelStatusEntry {
  model: string;
  name: string;
  size_mb: number;
  downloaded: boolean;
}

interface GpuStatus {
  cuda_available: boolean;
  metal_available: boolean;
  system_info: string;
}

interface TranscriptionSegment {
  id: string;
  text: string;
  timestamp_offset_ms: number;
  duration_ms: number;
  audio_path?: string;
}

// =============================================================================
// Settings persistence
// =============================================================================

/** Mirror of the Rust AgcConfig struct. */
interface AgcConfig {
  enabled: boolean;
  target_level_db: number;
  attack_time_ms: number;
  release_time_ms: number;
  min_gain_db: number;
  max_gain_db: number;
  gate_threshold_db: number;
}

/** Mirror of the Rust EngineConfig struct (snake_case matches serde output). */
interface EngineConfig {
  model: string;
  recording_mode: "mixed" | "echo_cancel";
  mic_gain_db: number;
  vad_voiced_threshold_db: number;
  vad_whisper_threshold_db: number;
  vad_voiced_onset_ms: number;
  vad_whisper_onset_ms: number;
  segment_max_duration_ms: number;
  segment_word_break_grace_ms: number;
  segment_lookback_ms: number;
  transcription_queue_capacity: number;
  viz_frame_interval_ms: number;
  word_break_segmentation_enabled: boolean;
  agc: AgcConfig;
}

/** Mirror of the Rust DemoConfig struct for JSON file persistence. */
interface DemoConfig {
  model: string;
  transcription_enabled: boolean;
  auto_transcription_enabled: boolean;
  aec_enabled: boolean;
  primary_device_id: string;
  secondary_device_id: string;
  mic_gain_db: number;
  vad_voiced_threshold_db: number;
  vad_whisper_threshold_db: number;
  vad_voiced_onset_ms: number;
  vad_whisper_onset_ms: number;
  segment_max_duration_ms: number;
  segment_word_break_grace_ms: number;
  segment_lookback_ms: number;
  transcription_queue_capacity: number;
  viz_frame_interval_ms: number;
  word_break_segmentation_enabled: boolean;
  audio_output_device_id: string;
  agc_enabled: boolean;
  agc_target_level_db: number;
  agc_gate_threshold_db: number;
}

async function loadDemoConfig(): Promise<DemoConfig> {
  try {
    return await invoke<DemoConfig>("load_demo_config");
  } catch (e) {
    console.error("Failed to load demo config:", e);
    return defaultDemoConfig();
  }
}

function defaultDemoConfig(): DemoConfig {
  return {
    model: "base_en",
    transcription_enabled: true,
    auto_transcription_enabled: false,
    aec_enabled: false,
    primary_device_id: "",
    secondary_device_id: "",
    mic_gain_db: 0.0,
    vad_voiced_threshold_db: -42.0,
    vad_whisper_threshold_db: -52.0,
    vad_voiced_onset_ms: 80,
    vad_whisper_onset_ms: 120,
    segment_max_duration_ms: 4000,
    segment_word_break_grace_ms: 750,
    segment_lookback_ms: 200,
    transcription_queue_capacity: 8,
    viz_frame_interval_ms: 16,
    word_break_segmentation_enabled: true,
    audio_output_device_id: "",
    agc_enabled: false,
    agc_target_level_db: -18.0,
    agc_gate_threshold_db: -50.0,
  };
}

async function saveDemoConfig(config: DemoConfig): Promise<void> {
  try {
    await invoke("save_demo_config", { config });
  } catch (e) {
    console.error("Failed to save demo config:", e);
  }
}

// =============================================================================
// State
// =============================================================================

let isRecording = false;
let isPlayingBack = false;
let activeDocumentPath: string | null = null;
let waveformRenderer: WaveformRenderer;
let spectrogramRenderer: SpectrogramRenderer;
let speechActivityRenderer: SpeechActivityRenderer;

// Demo config loaded from backend
let demoConfig: DemoConfig;
let transcriptionEnabled: boolean;
let autoTranscriptionEnabled: boolean;
let aecEnabled: boolean;

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
const appTitle = document.querySelector("header h1")!;
const btnCapture = document.getElementById("btn-capture") as HTMLButtonElement;
const btnOpenFile = document.getElementById(
  "btn-open-file"
) as HTMLButtonElement;
const btnPlay = document.getElementById(
  "btn-play"
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
const btnScrollBack = document.getElementById("btn-scroll-back") as HTMLButtonElement;
const btnScrollFwd = document.getElementById("btn-scroll-fwd") as HTMLButtonElement;
const btnScrollLive = document.getElementById("btn-scroll-live") as HTMLButtonElement;

// =============================================================================
// Document Model
// =============================================================================

const APP_TITLE_BASE = "VTX Engine Demo";

function setActiveDocument(path: string | null) {
  activeDocumentPath = path;
  if (path) {
    // Extract just the filename from the full path
    const filename = path.replace(/\\/g, "/").split("/").pop() ?? path;
    appTitle.textContent = `${APP_TITLE_BASE}: ${filename}`;
    document.title = `${APP_TITLE_BASE}: ${filename}`;
  } else {
    appTitle.textContent = APP_TITLE_BASE;
    document.title = APP_TITLE_BASE;
  }
  // Enable Play only when a document is open and not recording
  btnPlay.disabled = !path || isRecording;
}

// =============================================================================
// Initialization
// =============================================================================

async function init() {
  // Load persisted config from backend
  demoConfig = await loadDemoConfig();
  transcriptionEnabled = demoConfig.transcription_enabled;
  autoTranscriptionEnabled = demoConfig.auto_transcription_enabled;
  aecEnabled = demoConfig.aec_enabled;

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
    await applyPersistedConfigToBackend();
    await checkModelStatus();
    await checkGpuStatus();

    // Sync toggle states to the backend after engine is ready
    await syncTogglesToBackend();

    statusText.textContent = "Ready";
  }, 1000);
}

async function applyPersistedConfigToBackend() {
  try {
    await invoke("set_engine_config", {
      config: demoConfigToEngineConfig(demoConfig),
    });
  } catch (e) {
    console.error("Failed to apply persisted config:", e);
  }
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
  try {
    // Sync AEC / recording_mode — fetch current config and update only that field
    // so other config values (model, thresholds, etc.) are preserved.
    const cfg = await invoke<EngineConfig>("get_engine_config");
    cfg.recording_mode = aecEnabled ? "echo_cancel" : "mixed";
    await invoke("set_engine_config", { config: cfg });
  } catch (e) {
    console.error("Failed to sync recording_mode:", e);
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

  // Wire scroll interactions on the speech activity canvas.
  setupSpeechActivityScroll(speechCanvas);

  // Handle window resize
  window.addEventListener("resize", () => {
    waveformRenderer.resize();
    spectrogramRenderer.resize();
    speechActivityRenderer.resize();
  });
}

/**
 * Wire wheel and pointer-drag scroll interactions directly on the speech
 * activity canvas.  Handlers live here (not inside the renderer) so we can
 * call addEventListener with { passive: false } from a known-non-passive
 * context and reliably call preventDefault() to stop the parent <main>
 * scroll container from consuming the events.
 */
function setupSpeechActivityScroll(canvas: HTMLCanvasElement): void {
  canvas.style.touchAction = "none";
  canvas.style.cursor = "grab";

  // ---------------------------------------------------------------------------
  // Wheel scroll
  // ---------------------------------------------------------------------------
  canvas.addEventListener(
    "wheel",
    (e: WheelEvent) => {
      e.preventDefault();
      e.stopPropagation();

      // Prefer horizontal axis (trackpad swipe); fall back to vertical
      // inverted (scroll up = go back in time = positive frame offset).
      const raw =
        Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : -e.deltaY;

      // Normalise to logical pixels.
      let pixels: number;
      switch (e.deltaMode) {
        case 1: pixels = raw * 20; break;   // DOM_DELTA_LINE
        case 2: pixels = raw * 500; break;  // DOM_DELTA_PAGE
        default: pixels = raw; break;       // DOM_DELTA_PIXEL
      }

      // Scale pixels → frames (one canvas-width of drag = bufferSize frames).
      const rect = canvas.getBoundingClientRect();
      const framesPerPixel = speechActivityRenderer.bufferFrames / Math.max(rect.width, 1);
      speechActivityRenderer.scrollAccum += pixels * framesPerPixel;
      const whole = Math.trunc(speechActivityRenderer.scrollAccum);
      if (whole !== 0) {
        speechActivityRenderer.scrollAccum -= whole;
        speechActivityRenderer.scrollBy(whole);
      }
    },
    { passive: false }
  );

  // ---------------------------------------------------------------------------
  // Pointer drag
  // ---------------------------------------------------------------------------
  let isDragging = false;
  let dragLastX = 0;

  canvas.addEventListener("pointerdown", (e: PointerEvent) => {
    e.preventDefault();
    isDragging = true;
    dragLastX = e.clientX;
    speechActivityRenderer.scrollAccum = 0;
    canvas.setPointerCapture(e.pointerId);
    canvas.style.cursor = "grabbing";
  });

  canvas.addEventListener("pointermove", (e: PointerEvent) => {
    if (!isDragging) return;
    e.preventDefault();
    const dx = e.clientX - dragLastX;
    dragLastX = e.clientX;
    // Drag left (negative dx) → scroll into history (positive frame delta).
    // The graph moves with the finger: content under the pointer stays put,
    // so a leftward drag reveals older data on the right side.
    const rect = canvas.getBoundingClientRect();
    const framesPerPixel = speechActivityRenderer.bufferFrames / Math.max(rect.width, 1);
    speechActivityRenderer.scrollAccum += dx * framesPerPixel;
    const whole = Math.trunc(speechActivityRenderer.scrollAccum);
    if (whole !== 0) {
      speechActivityRenderer.scrollAccum -= whole;
      speechActivityRenderer.scrollBy(whole);
    }
  });

  const endDrag = (e: PointerEvent) => {
    if (!isDragging) return;
    e.preventDefault();
    isDragging = false;
    canvas.style.cursor = "grab";
  };
  canvas.addEventListener("pointerup", endDrag);
  canvas.addEventListener("pointercancel", endDrag);
}

function setupEventListeners() {
  btnCapture.addEventListener("click", toggleRecording);
  btnOpenFile.addEventListener("click", openWavFile);
  btnPlay.addEventListener("click", togglePlayback);
  btnDownloadModel.addEventListener("click", downloadModel);
  transcriptionToggle.addEventListener("change", onTranscriptionToggle);
  autoTranscriptionToggle.addEventListener("change", onAutoTranscriptionToggle);
  aecToggle.addEventListener("change", onAecToggle);

  // Save device selections on change
  deviceSelect.addEventListener("change", async () => {
    demoConfig.primary_device_id = deviceSelect.value;
    await saveDemoConfig(demoConfig);
  });
  deviceSelect2.addEventListener("change", async () => {
    demoConfig.secondary_device_id = deviceSelect2.value;
    await saveDemoConfig(demoConfig);
  });

  // Speech activity scroll buttons
  // Each click scrolls by 1/4 of the visible window (bufferSize/4 frames).
  btnScrollBack.addEventListener("click", () => {
    speechActivityRenderer.scrollBy(Math.max(1, Math.round(speechActivityRenderer.bufferFrames / 4)));
  });
  btnScrollFwd.addEventListener("click", () => {
    speechActivityRenderer.scrollBy(-Math.max(1, Math.round(speechActivityRenderer.bufferFrames / 4)));
  });
  btnScrollLive.addEventListener("click", () => {
    speechActivityRenderer.resetToLive();
  });

  // Configuration panel
  setupConfigPanelListeners();
}

async function setupBackendListeners() {
  // Visualization data
  let lastSampleRate = 0;
  let lastFrameIntervalMs = 0;
  await listen<VisualizationPayload>("visualization-data", (event) => {
    const data = event.payload;

    // Keep renderers calibrated to the actual audio source parameters so that
    // time-axis labels reflect real elapsed time.
    if (data.sample_rate && data.sample_rate !== lastSampleRate) {
      lastSampleRate = data.sample_rate;
      spectrogramRenderer.configure(data.sample_rate);
    }
    if (data.frame_interval_ms && data.frame_interval_ms !== lastFrameIntervalMs) {
      lastFrameIntervalMs = data.frame_interval_ms;
      speechActivityRenderer.configure(data.frame_interval_ms);
    }

    // Push waveform samples
    if (data.waveform && data.waveform.length > 0) {
      waveformRenderer.pushSamples(data.waveform);
    }

    // Push spectrogram columns (one per completed FFT window in this chunk)
    if (data.spectrogram) {
      for (const col of data.spectrogram) {
        spectrogramRenderer.pushColumn(col.colors);
      }
    }

    // Push speech metrics
    if (data.speech_metrics) {
      speechActivityRenderer.pushMetrics(data.speech_metrics);
    }
  });

  // Transcription results (live capture / VAD mode)
  await listen<TranscriptionSegment>("transcription-complete", (event) => {
    addTranscriptionResult(event.payload);
  });

  // Transcription segments (file playback VAD mode emits these)
  await listen<TranscriptionSegment>("transcription-segment", (event) => {
    addTranscriptionResult(event.payload);
  });

  // File playback complete
  await listen("playback-complete", () => {
    onPlaybackComplete();
  });

  // Capture state changes
  await listen<{ capturing: boolean; error: string | null }>(
    "capture-state-changed",
    (event) => {
      if (event.payload.error) {
        statusText.textContent = `Error: ${event.payload.error}`;
      } else if (event.payload.capturing && isRecording) {
        statusText.textContent = "Capturing...";
      } else if (!event.payload.capturing && !isPlayingBack) {
        statusText.textContent = "Ready";
      }
    }
  );

  // Speech events
  await listen("speech-started", () => {
    statusText.textContent = "Speaking...";
  });

  await listen("speech-ended", () => {
    if (isRecording || isPlayingBack) {
      statusText.textContent = isRecording ? "Capturing..." : "Playing...";
    }
    // In auto-transcription mode, speech-ended means the audio segment was
    // just split and submitted to the transcription queue.  Mark this moment
    // on the speech activity visualization.
    if (autoTranscriptionEnabled) {
      speechActivityRenderer.markSegmentSubmitted();
    }
  });

  // Model download progress — handles both legacy engine events (plain number
  // payload) and new config-panel events ({ model, progress } payload).
  await listen<number | { model: string; progress: number }>("model-download-progress", (event) => {
    if (typeof event.payload === "number") {
      // Legacy engine download event (plain percent number)
      modelStatusEl.textContent = `Downloading model: ${event.payload}%`;
    } else {
      // Config-panel model download event
      const { model, progress } = event.payload;
      downloadingModels.set(model, progress);
      if (progress === 100) {
        downloadingModels.delete(model);
        fetchAndRenderModelList();
      } else {
        renderModelList();
      }
    }
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

  await listen<{ model: string; error: string }>(
    "model-download-error",
    (event) => {
      downloadingModels.delete(event.payload.model);
      modelErrors.set(event.payload.model, event.payload.error);
      renderModelList();
    }
  );
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
    if (demoConfig.primary_device_id && allDeviceIds.includes(demoConfig.primary_device_id)) {
      deviceSelect.value = demoConfig.primary_device_id;
    }
    if (demoConfig.secondary_device_id === "" || allDeviceIds.includes(demoConfig.secondary_device_id)) {
      deviceSelect2.value = demoConfig.secondary_device_id;
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
    // Always start a recording session so the captured audio is saved to a WAV
    // file regardless of mode. In auto-transcription mode the backend also runs
    // VAD-driven segmentation in parallel; in PTT mode the whole session is
    // submitted as one segment on stop.
    await invoke("start_recording");
    isRecording = true;
    btnCapture.textContent = "Stop";
    btnCapture.classList.add("recording");
    deviceSelect.disabled = true;
    deviceSelect2.disabled = true;
    aecToggle.disabled = true;
    autoTranscriptionToggle.disabled = true;
    btnPlay.disabled = true;
    statusText.textContent = "Capturing...";

    // Reset visualization and transcription output for the new session
    waveformRenderer.clear();
    spectrogramRenderer.clear();
    speechActivityRenderer.clear();
    clearTranscriptionOutput();

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
    // In PTT mode, finalize the accumulated recording so it is submitted for
    // transcription as a single segment before stopping.
    if (!autoTranscriptionEnabled && transcriptionEnabled) {
      try {
        await invoke("finalize_segment");
      } catch (e) {
        console.error("Failed to finalize segment:", e);
      }
    }

    // Always stop the recording session (saves the WAV in both modes).
    const savedPath = await invoke<string | null>("stop_recording");

    await invoke("stop_capture");
    isRecording = false;
    btnCapture.textContent = "Record";
    btnCapture.classList.remove("recording");
    deviceSelect.disabled = false;
    deviceSelect2.disabled = false;
    aecToggle.disabled = false;
    autoTranscriptionToggle.disabled = false;
    statusText.textContent = "Ready";

    // Set the saved recording as the active document
    if (savedPath) {
      setActiveDocument(savedPath);
    } else {
      // Re-enable Play if a document was already open
      btnPlay.disabled = !activeDocumentPath;
    }

    // Stop renderers
    waveformRenderer.stop();
    spectrogramRenderer.stop();
    speechActivityRenderer.stop();
  } catch (e) {
    console.error("Failed to stop capture:", e);
  }
}

// =============================================================================
// File Playback (Open / Reprocess)
// =============================================================================

/** Start playing a file through the engine pipeline and update UI state.
 *
 * Audible output comes from the engine's WASAPI render endpoint — no browser
 * audio element is used.  The engine reprocesses from the raw recording with
 * the current processing settings (mic gain, AGC, etc.).
 */
async function startFilePlayback(filePath: string) {
  // Stop any previous engine pipeline playback.
  await invoke("stop_playback").catch(() => {});

  clearTranscriptionOutput();
  setPlayingBack(true);
  statusText.textContent = "Playing...";

  const pttMode = !autoTranscriptionEnabled;

  try {
    await invoke("open_file", { path: filePath, pttMode });
    // Engine pipeline runs in the background; onPlaybackComplete() fires via event.
  } catch (e) {
    console.error("Playback failed:", e);
    statusText.textContent = `Playback error: ${e}`;
    setPlayingBack(false);
    return;
  }
}

function setPlayingBack(active: boolean) {
  isPlayingBack = active;
  btnOpenFile.disabled = active;
  btnCapture.disabled = active || !deviceSelect.value;
  // Play button stays enabled during playback so the user can stop it;
  // it switches label and style to indicate the stop action.
  btnPlay.disabled = !activeDocumentPath || isRecording;
  btnPlay.textContent = active ? "\u25A0 Stop" : "\u25B6 Play";
  btnPlay.classList.toggle("playing", active);
  if (active) {
    // Reset then start renderers so they show fresh data from this playback.
    waveformRenderer.clear();
    spectrogramRenderer.clear();
    speechActivityRenderer.clear();
    waveformRenderer.start();
    spectrogramRenderer.start();
    speechActivityRenderer.start();
  }
}

function onPlaybackComplete() {
  isPlayingBack = false;
  waveformRenderer.stop();
  spectrogramRenderer.stop();
  speechActivityRenderer.stop();
  btnPlay.textContent = "\u25B6 Play";
  btnPlay.classList.remove("playing");
  btnOpenFile.disabled = false;
  btnCapture.disabled = !deviceSelect.value;
  btnPlay.disabled = !activeDocumentPath;
  statusText.textContent = "Ready";
}

async function openWavFile() {
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: "WAV Audio", extensions: ["wav"] }],
    });

    if (!selected) return;

    const filePath = typeof selected === "string" ? selected : selected;
    setActiveDocument(filePath);
    await startFilePlayback(filePath);
  } catch (e) {
    console.error("File dialog error:", e);
  }
}

async function togglePlayback() {
  if (isPlayingBack) {
    // Stop active playback (engine handles render endpoint cleanup).
    await invoke("stop_playback").catch(() => {});
    onPlaybackComplete();
  } else if (activeDocumentPath) {
    await startFilePlayback(activeDocumentPath);
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
    // Display the model name from the current engine config
    try {
      const cfg = await invoke<EngineConfig>("get_engine_config");
      const models = await invoke<ModelStatusEntry[]>("get_model_status");
      const entry = models.find((m) => m.model === cfg.model);
      modelNameEl.textContent = entry ? entry.name : cfg.model;
      modelNameEl.className = "status-badge badge-model";
    } catch {
      // Fall back to parsing the path
      const parts = status.path.replace(/\\/g, "/").split("/");
      const filename = parts[parts.length - 1] ?? status.path;
      const modelName = filename.replace(/\.bin$/, "").replace(/^ggml-/, "");
      modelNameEl.textContent = modelName;
      modelNameEl.className = "status-badge badge-model";
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
  demoConfig.transcription_enabled = transcriptionEnabled;
  demoConfig.primary_device_id = deviceSelect.value;
  demoConfig.secondary_device_id = deviceSelect2.value;
  await saveDemoConfig(demoConfig);
  try {
    await invoke("set_transcription_enabled", { enabled: transcriptionEnabled });
  } catch (e) {
    console.error("Failed to set transcription enabled:", e);
    // Revert the toggle on failure
    transcriptionEnabled = !transcriptionEnabled;
    transcriptionToggle.checked = transcriptionEnabled;
    demoConfig.transcription_enabled = transcriptionEnabled;
    await saveDemoConfig(demoConfig);
  }
}

async function onAutoTranscriptionToggle() {
  autoTranscriptionEnabled = autoTranscriptionToggle.checked;
  demoConfig.auto_transcription_enabled = autoTranscriptionEnabled;
  demoConfig.primary_device_id = deviceSelect.value;
  demoConfig.secondary_device_id = deviceSelect2.value;
  await saveDemoConfig(demoConfig);
  // auto-transcription OFF means PTT mode ON (manual submission on stop)
  await invoke("set_ptt_mode", { enabled: !autoTranscriptionEnabled }).catch((e) => {
    console.error("Failed to set PTT mode:", e);
  });
}

async function onAecToggle() {
  aecEnabled = aecToggle.checked;
  demoConfig.aec_enabled = aecEnabled;
  demoConfig.primary_device_id = deviceSelect.value;
  demoConfig.secondary_device_id = deviceSelect2.value;
  await saveDemoConfig(demoConfig);
  // Persist the recording mode to the engine so it takes effect on the next
  // start_capture call. Fetch the current config to avoid clobbering other
  // settings, then update only recording_mode.
  invoke<EngineConfig>("get_engine_config")
    .then((cfg) => {
      cfg.recording_mode = aecEnabled ? "echo_cancel" : "mixed";
      return invoke("set_engine_config", { config: cfg });
    })
    .catch((e) => {
      console.error("Failed to update recording_mode:", e);
    });
}

// =============================================================================
// Transcription Output
// =============================================================================

function clearTranscriptionOutput() {
  transcriptionOutput.innerHTML = "";
}

function addTranscriptionResult(result: TranscriptionSegment) {
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
// Configuration Panel
// =============================================================================

// DOM refs — config modal elements
const btnConfig = document.getElementById("btn-config") as HTMLButtonElement;
const configBackdrop = document.getElementById("config-backdrop") as HTMLDivElement;
const btnConfigClose = document.getElementById("btn-config-close") as HTMLButtonElement;
const btnConfigSave = document.getElementById("btn-config-save") as HTMLButtonElement;
const btnConfigReset = document.getElementById("btn-config-reset") as HTMLButtonElement;
const configCaptureWarning = document.getElementById("config-capture-warning") as HTMLDivElement;

// Audio Input
const cfgMicGain = document.getElementById("cfg-mic-gain") as HTMLInputElement;
const cfgMicGainDisplay = document.getElementById("cfg-mic-gain-display") as HTMLSpanElement;
const cfgAgcEnabled = document.getElementById("cfg-agc-enabled") as HTMLInputElement;
const cfgAgcTargetLevel = document.getElementById("cfg-agc-target-level") as HTMLInputElement;
const cfgAgcTargetLevelDisplay = document.getElementById("cfg-agc-target-level-display") as HTMLSpanElement;
const cfgAgcGateThreshold = document.getElementById("cfg-agc-gate-threshold") as HTMLInputElement;
const cfgAgcGateThresholdDisplay = document.getElementById("cfg-agc-gate-threshold-display") as HTMLSpanElement;

// Voice Detection
const cfgVadVoicedThreshold = document.getElementById("cfg-vad-voiced-threshold") as HTMLInputElement;
const cfgVadWhisperThreshold = document.getElementById("cfg-vad-whisper-threshold") as HTMLInputElement;
const cfgVadVoicedOnset = document.getElementById("cfg-vad-voiced-onset") as HTMLInputElement;
const cfgVadWhisperOnset = document.getElementById("cfg-vad-whisper-onset") as HTMLInputElement;

// Segmentation
const cfgSegmentMaxDuration = document.getElementById("cfg-segment-max-duration") as HTMLInputElement;
const cfgSegmentGrace = document.getElementById("cfg-segment-grace") as HTMLInputElement;
const cfgSegmentLookback = document.getElementById("cfg-segment-lookback") as HTMLInputElement;
const cfgWordBreakSegmentation = document.getElementById("cfg-word-break-segmentation") as HTMLInputElement;
const cfgQueueCapacity = document.getElementById("cfg-queue-capacity") as HTMLInputElement;

// Visualization
const cfgVizFrameInterval = document.getElementById("cfg-viz-frame-interval") as HTMLInputElement;

// Audio Output
const cfgOutputDevice = document.getElementById("cfg-output-device") as HTMLSelectElement;
const cfgOutputSupported = document.getElementById("cfg-output-supported") as HTMLDivElement;
const cfgOutputUnsupported = document.getElementById("cfg-output-unsupported") as HTMLDivElement;

// Model
const cfgModelList = document.getElementById("cfg-model-list") as HTMLDivElement;
const cfgModelWarning = document.getElementById("cfg-model-warning") as HTMLDivElement;

// Model state
let modelStatus: ModelStatusEntry[] = [];
let selectedModel: string = "base_en";
let downloadingModels: Map<string, number> = new Map();
let modelErrors: Map<string, string> = new Map();

/** Populate form fields from an EngineConfig object. */
function populateConfigForm(cfg: EngineConfig): void {
  selectedModel = cfg.model;
  cfgMicGain.value = String(cfg.mic_gain_db);
  updateGainDisplay(cfg.mic_gain_db);
  // AGC
  cfgAgcEnabled.checked = cfg.agc.enabled;
  cfgAgcTargetLevel.value = String(cfg.agc.target_level_db);
  cfgAgcTargetLevel.disabled = !cfg.agc.enabled;
  updateAgcTargetDisplay(cfg.agc.target_level_db);
  cfgAgcGateThreshold.value = String(cfg.agc.gate_threshold_db);
  cfgAgcGateThreshold.disabled = !cfg.agc.enabled;
  updateAgcGateThresholdDisplay(cfg.agc.gate_threshold_db);
  cfgVadVoicedThreshold.value = String(cfg.vad_voiced_threshold_db);
  cfgVadWhisperThreshold.value = String(cfg.vad_whisper_threshold_db);
  cfgVadVoicedOnset.value = String(cfg.vad_voiced_onset_ms);
  cfgVadWhisperOnset.value = String(cfg.vad_whisper_onset_ms);
  cfgSegmentMaxDuration.value = String(cfg.segment_max_duration_ms);
  cfgSegmentGrace.value = String(cfg.segment_word_break_grace_ms);
  cfgSegmentLookback.value = String(cfg.segment_lookback_ms);
  cfgWordBreakSegmentation.checked = cfg.word_break_segmentation_enabled;
  cfgQueueCapacity.value = String(cfg.transcription_queue_capacity);
  cfgVizFrameInterval.value = String(cfg.viz_frame_interval_ms);
}

/** Read form fields and build an EngineConfig object. */
function readConfigForm(): EngineConfig {
  return {
    model: selectedModel,
    // recording_mode is controlled by the AEC toggle on the main UI, not the
    // config panel. Derive it from the current aecEnabled state so saving the
    // config panel never accidentally resets it.
    recording_mode: aecEnabled ? "echo_cancel" : "mixed",
    mic_gain_db: parseFloat(cfgMicGain.value),
    agc: {
      enabled: cfgAgcEnabled.checked,
      target_level_db: parseFloat(cfgAgcTargetLevel.value),
      // Non-exposed fields use AgcConfig defaults.
      attack_time_ms: 10.0,
      release_time_ms: 200.0,
      min_gain_db: -6.0,
      max_gain_db: 30.0,
      gate_threshold_db: parseFloat(cfgAgcGateThreshold.value),
    },
    vad_voiced_threshold_db: parseFloat(cfgVadVoicedThreshold.value),
    vad_whisper_threshold_db: parseFloat(cfgVadWhisperThreshold.value),
    vad_voiced_onset_ms: parseInt(cfgVadVoicedOnset.value, 10),
    vad_whisper_onset_ms: parseInt(cfgVadWhisperOnset.value, 10),
    segment_max_duration_ms: parseInt(cfgSegmentMaxDuration.value, 10),
    segment_word_break_grace_ms: parseInt(cfgSegmentGrace.value, 10),
    segment_lookback_ms: parseInt(cfgSegmentLookback.value, 10),
    transcription_queue_capacity: parseInt(cfgQueueCapacity.value, 10),
    viz_frame_interval_ms: parseInt(cfgVizFrameInterval.value, 10),
    word_break_segmentation_enabled: cfgWordBreakSegmentation.checked,
  };
}

/** Update the live dB readout next to the mic gain slider (task 9.1). */
function updateGainDisplay(db: number): void {
  const sign = db > 0 ? "+" : "";
  cfgMicGainDisplay.textContent = `${sign}${db.toFixed(1)} dB`;
}

/** Update the live dBFS readout next to the AGC target level slider. */
function updateAgcTargetDisplay(db: number): void {
  cfgAgcTargetLevelDisplay.textContent = `${db.toFixed(1)} dBFS`;
}

/** Update the live dBFS readout next to the AGC gate threshold slider. */
function updateAgcGateThresholdDisplay(db: number): void {
  cfgAgcGateThresholdDisplay.textContent = `${db.toFixed(1)} dBFS`;
}

/** Populate the output device selector from the browser's device list. */
async function populateOutputDevices(savedId: string): Promise<void> {
  const sinkIdSupported = typeof (HTMLMediaElement.prototype as any).setSinkId === "function";
  if (!sinkIdSupported) {
    cfgOutputSupported.style.display = "none";
    cfgOutputUnsupported.style.display = "";
    return;
  }
  cfgOutputSupported.style.display = "";
  cfgOutputUnsupported.style.display = "none";

  try {
    // enumerateDevices() only returns full device labels after a getUserMedia
    // grant. Request a brief mic stream to obtain the permission token, then
    // immediately stop it so no audio is captured.
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      stream.getTracks().forEach((t) => t.stop());
    } catch {
      // Permission denied or no mic — labels may still be generic, proceed anyway.
    }

    const devices = await navigator.mediaDevices.enumerateDevices();
    const outputDevices = devices.filter((d) => d.kind === "audiooutput");

    cfgOutputDevice.innerHTML = "";
    const defaultOpt = document.createElement("option");
    defaultOpt.value = "";
    defaultOpt.textContent = "Default";
    cfgOutputDevice.appendChild(defaultOpt);

    for (const dev of outputDevices) {
      const opt = document.createElement("option");
      opt.value = dev.deviceId;
      opt.textContent = dev.label || `Output ${dev.deviceId.substring(0, 8)}`;
      cfgOutputDevice.appendChild(opt);
    }

    // Restore saved selection if it still exists
    if (savedId && Array.from(cfgOutputDevice.options).some((o) => o.value === savedId)) {
      cfgOutputDevice.value = savedId;
    }
  } catch (e) {
    console.error("Failed to enumerate output devices:", e);
  }
}

/** Fetch model status from backend and render the model list. */
async function fetchAndRenderModelList(): Promise<void> {
  try {
    modelStatus = await invoke<ModelStatusEntry[]>("get_model_status");
    renderModelList();
  } catch (e) {
    console.error("Failed to get model status:", e);
    cfgModelList.innerHTML = '<p class="config-note">Failed to load models</p>';
  }
}

/** Render the model list UI. */
function renderModelList(): void {
  cfgModelList.innerHTML = "";

  for (const entry of modelStatus) {
    const row = document.createElement("div");
    row.className = "config-model-row";
    if (entry.model === selectedModel && entry.downloaded) {
      row.classList.add("selected");
    }

    const isDownloading = downloadingModels.has(entry.model);
    const error = modelErrors.get(entry.model);

    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "model-select";
    radio.className = "config-model-radio";
    radio.checked = entry.model === selectedModel && entry.downloaded;
    radio.disabled = !entry.downloaded || isDownloading;
    radio.addEventListener("change", () => {
      if (entry.downloaded) {
        selectedModel = entry.model;
        renderModelList();
      }
    });

    const name = document.createElement("span");
    name.className = "config-model-name";
    name.textContent = entry.name;

    const size = document.createElement("span");
    size.className = "config-model-size";
    size.textContent = entry.size_mb >= 1024
      ? `${(entry.size_mb / 1024).toFixed(1)} GiB`
      : `${entry.size_mb} MiB`;

    const statusEl = document.createElement("span");
    statusEl.className = "config-model-status";
    if (isDownloading) {
      statusEl.classList.add("downloading");
      statusEl.textContent = `${downloadingModels.get(entry.model)}%`;
    } else if (error) {
      statusEl.classList.add("error");
      statusEl.textContent = "Error";
    } else if (entry.downloaded) {
      statusEl.classList.add("downloaded");
      statusEl.textContent = "Downloaded";
    } else {
      statusEl.classList.add("not-downloaded");
      statusEl.textContent = "Not downloaded";
    }

    const actions = document.createElement("div");
    actions.className = "config-model-actions";

    if (isDownloading) {
      const progress = document.createElement("div");
      progress.className = "config-model-progress";
      const fill = document.createElement("div");
      fill.className = "fill";
      fill.style.width = `${downloadingModels.get(entry.model)}%`;
      progress.appendChild(fill);
      actions.appendChild(progress);

      const cancelBtn = document.createElement("button");
      cancelBtn.className = "config-model-btn cancel";
      cancelBtn.textContent = "Cancel";
      cancelBtn.addEventListener("click", () => cancelModelDownload(entry.model));
      actions.appendChild(cancelBtn);
    } else if (error) {
      const errorEl = document.createElement("span");
      errorEl.className = "config-model-error";
      errorEl.textContent = error.length > 20 ? error.substring(0, 20) + "..." : error;
      actions.appendChild(errorEl);

      const retryBtn = document.createElement("button");
      retryBtn.className = "config-model-btn download";
      retryBtn.textContent = "Retry";
      retryBtn.addEventListener("click", () => startModelDownload(entry.model));
      actions.appendChild(retryBtn);
    } else if (!entry.downloaded) {
      const downloadBtn = document.createElement("button");
      downloadBtn.className = "config-model-btn download";
      downloadBtn.textContent = "Download";
      downloadBtn.addEventListener("click", () => startModelDownload(entry.model));
      actions.appendChild(downloadBtn);
    }

    row.appendChild(radio);
    row.appendChild(name);
    row.appendChild(size);
    row.appendChild(statusEl);
    row.appendChild(actions);
    cfgModelList.appendChild(row);
  }
}

/** Start downloading a model. */
async function startModelDownload(model: string): Promise<void> {
  downloadingModels.set(model, 0);
  modelErrors.delete(model);
  renderModelList();

  try {
    await invoke("download_model_by_name", { model });
  } catch (e) {
    console.error("Failed to start download:", e);
    downloadingModels.delete(model);
    modelErrors.set(model, String(e));
    renderModelList();
  }
}

/** Cancel a model download. */
async function cancelModelDownload(model: string): Promise<void> {
  try {
    await invoke("cancel_model_download", { model });
    downloadingModels.delete(model);
    renderModelList();
  } catch (e) {
    console.error("Failed to cancel download:", e);
  }
}

let escapeListener: ((e: KeyboardEvent) => void) | null = null;

/** Open the configuration panel. */
async function openConfigPanel(): Promise<void> {
  // Fetch current config from backend
  try {
    const cfg = await invoke<EngineConfig>("get_engine_config");
    populateConfigForm(cfg);
  } catch (e) {
    console.error("Failed to get engine config:", e);
    // Fall back to saved settings
    populateConfigForm(demoConfigToEngineConfig(demoConfig));
  }

  // Fetch model status
  await fetchAndRenderModelList();

  // Populate output devices
  await populateOutputDevices(demoConfig.audio_output_device_id);

  // Show capture warning if active
  configCaptureWarning.style.display = isRecording ? "" : "none";
  cfgModelWarning.style.display = isRecording ? "" : "none";

  // Show modal
  configBackdrop.style.display = "flex";
  configBackdrop.removeAttribute("aria-hidden");

  // Escape to close
  escapeListener = (e: KeyboardEvent) => {
    if (e.key === "Escape") closeConfigPanel();
  };
  document.addEventListener("keydown", escapeListener);

  // Focus the close button for accessibility
  btnConfigClose.focus();
}

/** Close the configuration panel. */
function closeConfigPanel(): void {
  configBackdrop.style.display = "none";
  configBackdrop.setAttribute("aria-hidden", "true");
  if (escapeListener) {
    document.removeEventListener("keydown", escapeListener);
    escapeListener = null;
  }
  btnConfig.focus();
}

/** Save the current form values, apply to backend, and close. */
async function saveConfig(): Promise<void> {
  console.log("saveConfig called");
  const cfg = readConfigForm();

  try {
    await invoke("set_engine_config", { config: cfg });
  } catch (e) {
    console.error("Failed to set engine config:", e);
  }

  // NOTE: Output device selection via setSinkId has been removed — audible
  // playback now goes through the engine's WASAPI render endpoint which uses
  // the system default output device.

  // Persist to JSON config file
  demoConfig = {
    ...demoConfig,
    model: cfg.model,
    mic_gain_db: cfg.mic_gain_db,
    agc_enabled: cfg.agc.enabled,
    agc_target_level_db: cfg.agc.target_level_db,
    agc_gate_threshold_db: cfg.agc.gate_threshold_db,
    vad_voiced_threshold_db: cfg.vad_voiced_threshold_db,
    vad_whisper_threshold_db: cfg.vad_whisper_threshold_db,
    vad_voiced_onset_ms: cfg.vad_voiced_onset_ms,
    vad_whisper_onset_ms: cfg.vad_whisper_onset_ms,
    segment_max_duration_ms: cfg.segment_max_duration_ms,
    segment_word_break_grace_ms: cfg.segment_word_break_grace_ms,
    segment_lookback_ms: cfg.segment_lookback_ms,
    transcription_queue_capacity: cfg.transcription_queue_capacity,
    viz_frame_interval_ms: cfg.viz_frame_interval_ms,
    word_break_segmentation_enabled: cfg.word_break_segmentation_enabled,
    audio_output_device_id: cfgOutputDevice.value,
  };
  await saveDemoConfig(demoConfig);

  // Update model name badge in status bar
  const modelEntry = modelStatus.find((m) => m.model === cfg.model);
  if (modelEntry) {
    modelNameEl.textContent = modelEntry.name;
    modelNameEl.className = "status-badge badge-model";
  }

  console.log("Calling closeConfigPanel");
  closeConfigPanel();
  console.log("closeConfigPanel called");
}

/** Reset all form fields to factory defaults without saving. */
function resetToDefaults(): void {
  const d = defaultDemoConfig();
  populateConfigForm(demoConfigToEngineConfig(d));
  // Also reset output device selector to default
  cfgOutputDevice.value = "";
}

/** Convert DemoConfig to EngineConfig. */
function demoConfigToEngineConfig(d: DemoConfig): EngineConfig {
  return {
    model: d.model,
    recording_mode: d.aec_enabled ? "echo_cancel" : "mixed",
    mic_gain_db: d.mic_gain_db,
    agc: {
      enabled: d.agc_enabled,
      target_level_db: d.agc_target_level_db,
      attack_time_ms: 10.0,
      release_time_ms: 200.0,
      min_gain_db: -6.0,
      max_gain_db: 30.0,
      gate_threshold_db: d.agc_gate_threshold_db,
    },
    vad_voiced_threshold_db: d.vad_voiced_threshold_db,
    vad_whisper_threshold_db: d.vad_whisper_threshold_db,
    vad_voiced_onset_ms: d.vad_voiced_onset_ms,
    vad_whisper_onset_ms: d.vad_whisper_onset_ms,
    segment_max_duration_ms: d.segment_max_duration_ms,
    segment_word_break_grace_ms: d.segment_word_break_grace_ms,
    segment_lookback_ms: d.segment_lookback_ms,
    transcription_queue_capacity: d.transcription_queue_capacity,
    viz_frame_interval_ms: d.viz_frame_interval_ms,
    word_break_segmentation_enabled: d.word_break_segmentation_enabled,
  };
}

/** Wire up config panel event listeners. */
function setupConfigPanelListeners(): void {
  btnConfig.addEventListener("click", openConfigPanel);
  btnConfigClose.addEventListener("click", closeConfigPanel);
  btnConfigSave.addEventListener("click", saveConfig);
  btnConfigReset.addEventListener("click", resetToDefaults);

  // Backdrop click closes panel (only when clicking the backdrop itself)
  configBackdrop.addEventListener("click", (e) => {
    if (e.target === configBackdrop) closeConfigPanel();
  });

  // Live dB readout on slider input (task 9.1)
  cfgMicGain.addEventListener("input", () => {
    updateGainDisplay(parseFloat(cfgMicGain.value));
  });

  // AGC: toggle slider enabled state when checkbox changes
  cfgAgcEnabled.addEventListener("change", () => {
    cfgAgcTargetLevel.disabled = !cfgAgcEnabled.checked;
    cfgAgcGateThreshold.disabled = !cfgAgcEnabled.checked;
  });

  // Live dBFS readout on AGC target level slider input
  cfgAgcTargetLevel.addEventListener("input", () => {
    updateAgcTargetDisplay(parseFloat(cfgAgcTargetLevel.value));
  });

  // Live dBFS readout on AGC gate threshold slider input
  cfgAgcGateThreshold.addEventListener("input", () => {
    updateAgcGateThresholdDisplay(parseFloat(cfgAgcGateThreshold.value));
  });
}

// =============================================================================
// Start
// =============================================================================

init();
