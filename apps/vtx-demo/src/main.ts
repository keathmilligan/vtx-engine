// vtx-engine Demo Application
//
// Demonstrates live audio capture with visualization, and WAV file transcription.

import { invoke, convertFileSrc } from "@tauri-apps/api/core";
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

const SETTINGS_KEY = "vtx-demo-settings";

/** Mirror of the Rust EngineConfig struct (snake_case matches serde output). */
interface EngineConfig {
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
}

interface AppSettings {
  transcriptionEnabled: boolean;
  autoTranscriptionEnabled: boolean;
  aecEnabled: boolean;
  primaryDeviceId: string;
  secondaryDeviceId: string;
  // Engine config fields (persisted as camelCase in localStorage)
  micGainDb: number;
  vadVoicedThresholdDb: number;
  vadWhisperThresholdDb: number;
  vadVoicedOnsetMs: number;
  vadWhisperOnsetMs: number;
  segmentMaxDurationMs: number;
  segmentWordBreakGraceMs: number;
  segmentLookbackMs: number;
  transcriptionQueueCapacity: number;
  vizFrameIntervalMs: number;
  wordBreakSegmentationEnabled: boolean;
  audioOutputDeviceId: string;
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
    // Engine config defaults (must match Rust EngineConfig defaults)
    micGainDb: 0.0,
    vadVoicedThresholdDb: -42.0,
    vadWhisperThresholdDb: -52.0,
    vadVoicedOnsetMs: 80,
    vadWhisperOnsetMs: 120,
    segmentMaxDurationMs: 4000,
    segmentWordBreakGraceMs: 750,
    segmentLookbackMs: 200,
    transcriptionQueueCapacity: 8,
    vizFrameIntervalMs: 16,
    wordBreakSegmentationEnabled: true,
    audioOutputDeviceId: "",
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
  // Merge with persisted settings so engine config fields are not lost
  // when saving only the toggle/device state.
  const persisted = loadSettings();
  return {
    ...persisted,
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
let isPlayingBack = false;
let activeDocumentPath: string | null = null;
let playbackAudio: HTMLAudioElement | null = null;
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
  deviceSelect.addEventListener("change", () => saveSettings(getCurrentSettings()));
  deviceSelect2.addEventListener("change", () => saveSettings(getCurrentSettings()));

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

/** Stop any in-progress audio element playback. */
function stopAudioElement() {
  if (playbackAudio) {
    playbackAudio.pause();
    playbackAudio.src = "";
    playbackAudio = null;
  }
}

/** Start playing a file through the engine pipeline and update UI state. */
async function startFilePlayback(filePath: string) {
  // Stop any previous playback (audio element and engine pipeline).
  stopAudioElement();
  await invoke("stop_playback").catch(() => {});

  clearTranscriptionOutput();
  setPlayingBack(true);
  statusText.textContent = "Playing...";

  // Prepare the audio element (but don't play yet).
  const audioUrl = convertFileSrc(filePath);
  playbackAudio = new Audio(audioUrl);

  // Apply saved output device selection (task 8.9)
  const sinkIdSupported = typeof (HTMLMediaElement.prototype as any).setSinkId === "function";
  if (sinkIdSupported) {
    const savedOutputId = loadSettings().audioOutputDeviceId;
    if (savedOutputId) {
      try {
        await (playbackAudio as any).setSinkId(savedOutputId);
      } catch (e) {
        console.warn("setSinkId failed on playback start:", e);
      }
    }
  }

  const pttMode = !autoTranscriptionEnabled;

  try {
    // Start the engine pipeline first so it is already processing audio by the
    // time the audio element begins playback. open_file() returns as soon as the
    // feeder thread is spawned, so the IPC round-trip latency becomes a head-start
    // for the pipeline rather than a lag behind the audio element.
    await invoke("open_file", { path: filePath, pttMode });
    // Engine pipeline runs in the background; onPlaybackComplete() fires via event.
  } catch (e) {
    console.error("Playback failed:", e);
    statusText.textContent = `Playback error: ${e}`;
    stopAudioElement();
    setPlayingBack(false);
    return;
  }

  // Start audible playback after the engine pipeline is running.
  playbackAudio.play().catch((e) => {
    console.error("Audio element playback failed:", e);
  });
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
  stopAudioElement();
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
    // Stop active playback.
    stopAudioElement();
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
  // auto-transcription OFF means PTT mode ON (manual submission on stop)
  await invoke("set_ptt_mode", { enabled: !autoTranscriptionEnabled }).catch((e) => {
    console.error("Failed to set PTT mode:", e);
  });
}

function onAecToggle() {
  aecEnabled = aecToggle.checked;
  saveSettings(getCurrentSettings());
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

/** Populate form fields from an EngineConfig object. */
function populateConfigForm(cfg: EngineConfig): void {
  cfgMicGain.value = String(cfg.mic_gain_db);
  updateGainDisplay(cfg.mic_gain_db);
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
    // recording_mode is controlled by the AEC toggle on the main UI, not the
    // config panel. Derive it from the current aecEnabled state so saving the
    // config panel never accidentally resets it.
    recording_mode: aecEnabled ? "echo_cancel" : "mixed",
    mic_gain_db: parseFloat(cfgMicGain.value),
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
    const s = loadSettings();
    populateConfigForm(settingsToEngineConfig(s));
  }

  // Populate output devices
  const savedSettings = loadSettings();
  await populateOutputDevices(savedSettings.audioOutputDeviceId);

  // Show capture warning if active
  configCaptureWarning.style.display = isRecording ? "" : "none";

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
  const cfg = readConfigForm();

  try {
    await invoke("set_engine_config", { config: cfg });
  } catch (e) {
    console.error("Failed to set engine config:", e);
  }

  // Apply output device selection via setSinkId
  const sinkIdSupported = typeof (HTMLMediaElement.prototype as any).setSinkId === "function";
  const selectedOutputId = cfgOutputDevice.value;
  if (sinkIdSupported && playbackAudio) {
    try {
      await (playbackAudio as any).setSinkId(selectedOutputId);
    } catch (e) {
      console.warn("setSinkId failed:", e);
    }
  }

  // Persist to localStorage
  const s = loadSettings();
  const updated: AppSettings = {
    ...s,
    micGainDb: cfg.mic_gain_db,
    vadVoicedThresholdDb: cfg.vad_voiced_threshold_db,
    vadWhisperThresholdDb: cfg.vad_whisper_threshold_db,
    vadVoicedOnsetMs: cfg.vad_voiced_onset_ms,
    vadWhisperOnsetMs: cfg.vad_whisper_onset_ms,
    segmentMaxDurationMs: cfg.segment_max_duration_ms,
    segmentWordBreakGraceMs: cfg.segment_word_break_grace_ms,
    segmentLookbackMs: cfg.segment_lookback_ms,
    transcriptionQueueCapacity: cfg.transcription_queue_capacity,
    vizFrameIntervalMs: cfg.viz_frame_interval_ms,
    wordBreakSegmentationEnabled: cfg.word_break_segmentation_enabled,
    audioOutputDeviceId: selectedOutputId,
  };
  saveSettings(updated);

  closeConfigPanel();
}

/** Reset all form fields to factory defaults without saving. */
function resetToDefaults(): void {
  const d = defaultSettings();
  populateConfigForm(settingsToEngineConfig(d));
  // Also reset output device selector to default
  cfgOutputDevice.value = "";
}

/** Convert AppSettings (camelCase) to EngineConfig (snake_case). */
function settingsToEngineConfig(s: AppSettings): EngineConfig {
  return {
    recording_mode: s.aecEnabled ? "echo_cancel" : "mixed",
    mic_gain_db: s.micGainDb,
    vad_voiced_threshold_db: s.vadVoicedThresholdDb,
    vad_whisper_threshold_db: s.vadWhisperThresholdDb,
    vad_voiced_onset_ms: s.vadVoicedOnsetMs,
    vad_whisper_onset_ms: s.vadWhisperOnsetMs,
    segment_max_duration_ms: s.segmentMaxDurationMs,
    segment_word_break_grace_ms: s.segmentWordBreakGraceMs,
    segment_lookback_ms: s.segmentLookbackMs,
    transcription_queue_capacity: s.transcriptionQueueCapacity,
    viz_frame_interval_ms: s.vizFrameIntervalMs,
    word_break_segmentation_enabled: s.wordBreakSegmentationEnabled,
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
}

// =============================================================================
// Start
// =============================================================================

init();
