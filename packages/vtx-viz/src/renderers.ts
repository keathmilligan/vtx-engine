// Canvas2D-based visualization renderers for vtx-engine
//
// All renderers read colors from CSS custom properties for theming.
// See styles.css for the default theme variables.

import { RingBuffer } from "./ring-buffer";
import type { SpeechMetrics } from "./types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function cssVar(name: string, fallback: string): string {
  return (
    getComputedStyle(document.documentElement).getPropertyValue(name).trim() ||
    fallback
  );
}

// ---------------------------------------------------------------------------
// WaveformRenderer
// ---------------------------------------------------------------------------

/** Full-size oscilloscope waveform with grid, labels, and glow effect. */
export class WaveformRenderer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private animationId: number | null = null;
  private ringBuffer: RingBuffer;
  private isActive = false;

  constructor(canvas: HTMLCanvasElement, bufferSize = 512) {
    this.canvas = canvas;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Could not get canvas 2D context");
    this.ctx = ctx;
    this.ringBuffer = new RingBuffer(bufferSize);
    this.setupCanvas();
  }

  private setupCanvas(): void {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.scale(dpr, dpr);
  }

  pushSamples(samples: number[]): void {
    this.ringBuffer.push(samples);
  }

  start(): void {
    if (this.isActive) return;
    this.isActive = true;
    this.animate();
  }

  stop(): void {
    this.isActive = false;
    if (this.animationId !== null) {
      cancelAnimationFrame(this.animationId);
      this.animationId = null;
    }
  }

  get active(): boolean {
    return this.isActive;
  }

  clear(): void {
    this.ringBuffer.clear();
    this.drawIdle();
  }

  resize(): void {
    this.setupCanvas();
    this.drawIdle();
  }

  drawIdle(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    this.ctx.fillStyle = cssVar("--vtx-waveform-bg", "#1e293b");
    this.ctx.fillRect(0, 0, width, height);
    this.drawGrid(width, height);
    const area = this.getDrawableArea();
    this.drawCenterLine(area);
  }

  private animate = (): void => {
    if (!this.isActive) return;
    this.draw();
    this.animationId = requestAnimationFrame(this.animate);
  };

  private draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    const samples = this.ringBuffer.getSamples();

    this.ctx.fillStyle = cssVar("--vtx-waveform-bg", "#1e293b");
    this.ctx.fillRect(0, 0, width, height);
    this.drawGrid(width, height);

    const area = this.getDrawableArea();
    if (samples.length === 0) {
      this.drawCenterLine(area);
      return;
    }

    const waveformColor = cssVar("--vtx-waveform-color", "#3b82f6");
    const glowColor = cssVar("--vtx-waveform-glow", "rgba(59, 130, 246, 0.5)");
    const centerY = area.y + area.height / 2;
    const amplitude = (area.height / 2 - 4) * 1.5;
    const pointCount = samples.length;

    this.ctx.beginPath();
    for (let i = 0; i < pointCount; i++) {
      const sample = samples[i] || 0;
      const x = area.x + (i / pointCount) * area.width;
      const clampedSample = Math.max(-1, Math.min(1, sample));
      const y = centerY - clampedSample * amplitude;
      if (i === 0) this.ctx.moveTo(x, y);
      else this.ctx.lineTo(x, y);
    }

    // Glow layer
    this.ctx.save();
    this.ctx.strokeStyle = glowColor;
    this.ctx.lineWidth = 6;
    this.ctx.filter = "blur(4px)";
    this.ctx.stroke();
    this.ctx.restore();

    // Main line
    this.ctx.strokeStyle = waveformColor;
    this.ctx.lineWidth = 2;
    this.ctx.stroke();
  }

  private drawGrid(width: number, height: number): void {
    const gridColor = cssVar("--vtx-waveform-grid", "rgba(255,255,255,0.08)");
    const textColor = cssVar("--vtx-waveform-text", "rgba(255,255,255,0.5)");
    const leftMargin = 40;
    const rightMargin = 8;
    const topMargin = 8;
    const bottomMargin = 20;
    const graphWidth = width - leftMargin - rightMargin;
    const graphHeight = height - topMargin - bottomMargin;

    this.ctx.strokeStyle = gridColor;
    this.ctx.lineWidth = 1;

    for (let i = 0; i <= 8; i++) {
      const y = topMargin + (graphHeight / 8) * i;
      this.ctx.beginPath();
      this.ctx.moveTo(leftMargin, y);
      this.ctx.lineTo(leftMargin + graphWidth, y);
      this.ctx.stroke();
    }

    for (let i = 0; i <= 16; i++) {
      const x = leftMargin + (graphWidth / 16) * i;
      this.ctx.beginPath();
      this.ctx.moveTo(x, topMargin);
      this.ctx.lineTo(x, topMargin + graphHeight);
      this.ctx.stroke();
    }

    this.ctx.fillStyle = textColor;
    this.ctx.font = "10px system-ui, sans-serif";
    this.ctx.textAlign = "right";
    this.ctx.textBaseline = "middle";

    const yLabels = ["1.0", "0.5", "0", "-0.5", "-1.0"];
    const yPositions = [0, 0.25, 0.5, 0.75, 1];
    for (let i = 0; i < yLabels.length; i++) {
      this.ctx.fillText(
        yLabels[i],
        leftMargin - 4,
        topMargin + yPositions[i] * graphHeight
      );
    }

    this.ctx.textAlign = "center";
    this.ctx.textBaseline = "top";
    const timeLabels = ["-80ms", "-60ms", "-40ms", "-20ms", "0"];
    for (let i = 0; i < timeLabels.length; i++) {
      const x = leftMargin + (graphWidth / (timeLabels.length - 1)) * i;
      this.ctx.fillText(timeLabels[i], x, topMargin + graphHeight + 4);
    }
  }

  private getDrawableArea() {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    return {
      x: 40,
      y: 8,
      width: width - 40 - 8,
      height: height - 8 - 20,
    };
  }

  private drawCenterLine(area: {
    x: number;
    y: number;
    width: number;
    height: number;
  }): void {
    this.ctx.strokeStyle = cssVar("--vtx-waveform-line", "#475569");
    this.ctx.lineWidth = 1;
    this.ctx.beginPath();
    const centerY = area.y + area.height / 2;
    this.ctx.moveTo(area.x, centerY);
    this.ctx.lineTo(area.x + area.width, centerY);
    this.ctx.stroke();
  }
}

// ---------------------------------------------------------------------------
// SpectrogramRenderer
// ---------------------------------------------------------------------------

/** Scrolling spectrogram using pixel-level ImageData manipulation. */
export class SpectrogramRenderer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private offscreenCanvas: HTMLCanvasElement;
  private offscreenCtx: CanvasRenderingContext2D;
  private animationId: number | null = null;
  private isActive = false;
  private imageData: ImageData | null = null;
  private columnQueue: number[][] = [];
  private maxQueueSize = 60;

  private readonly leftMargin = 40;
  private readonly rightMargin = 8;
  private readonly topMargin = 8;
  private readonly bottomMargin = 20;

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Could not get canvas 2D context");
    this.ctx = ctx;

    this.offscreenCanvas = document.createElement("canvas");
    const offCtx = this.offscreenCanvas.getContext("2d");
    if (!offCtx) throw new Error("Could not get offscreen canvas 2D context");
    this.offscreenCtx = offCtx;

    this.setupCanvas();
  }

  private setupCanvas(): void {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.scale(dpr, dpr);

    const drawableWidth = Math.floor(
      rect.width - this.leftMargin - this.rightMargin
    );
    const drawableHeight = Math.floor(
      rect.height - this.topMargin - this.bottomMargin
    );
    this.offscreenCanvas.width = drawableWidth * dpr;
    this.offscreenCanvas.height = drawableHeight * dpr;

    this.imageData = this.offscreenCtx.createImageData(
      drawableWidth * dpr,
      drawableHeight * dpr
    );
    this.fillBackground();
  }

  private fillBackground(): void {
    if (!this.imageData) return;
    const data = this.imageData.data;
    const bgHex = cssVar("--vtx-waveform-bg", "#0a0f1a");
    const rgb = this.parseHex(bgHex);
    for (let i = 0; i < data.length; i += 4) {
      data[i] = rgb.r;
      data[i + 1] = rgb.g;
      data[i + 2] = rgb.b;
      data[i + 3] = 255;
    }
  }

  private parseHex(hex: string) {
    const h = hex.replace("#", "");
    return {
      r: parseInt(h.substring(0, 2), 16) || 0,
      g: parseInt(h.substring(2, 4), 16) || 0,
      b: parseInt(h.substring(4, 6), 16) || 0,
    };
  }

  pushColumn(colors: number[]): void {
    if (this.columnQueue.length < this.maxQueueSize) {
      this.columnQueue.push(colors);
    } else {
      this.columnQueue.shift();
      this.columnQueue.push(colors);
    }
  }

  start(): void {
    if (this.isActive) return;
    this.isActive = true;
    this.animate();
  }

  stop(): void {
    this.isActive = false;
    if (this.animationId !== null) {
      cancelAnimationFrame(this.animationId);
      this.animationId = null;
    }
  }

  get active(): boolean {
    return this.isActive;
  }

  clear(): void {
    this.columnQueue = [];
    this.fillBackground();
    this.drawIdle();
  }

  resize(): void {
    this.setupCanvas();
    this.drawIdle();
  }

  drawIdle(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    this.ctx.fillStyle = cssVar("--vtx-waveform-bg", "#1e293b");
    this.ctx.fillRect(0, 0, width, height);
    this.drawGrid(width, height);
  }

  private animate = (): void => {
    if (!this.isActive) return;
    this.draw();
    this.animationId = requestAnimationFrame(this.animate);
  };

  private draw(): void {
    if (!this.imageData) return;
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;

    const columnsToProcess = Math.min(
      this.columnQueue.length,
      Math.max(2, Math.ceil(this.columnQueue.length / 4))
    );

    for (let i = 0; i < columnsToProcess; i++) {
      const column = this.columnQueue.shift()!;
      this.scrollLeft();
      this.drawColumn(column);
    }

    const bgColor = cssVar("--vtx-waveform-bg", "#000032");
    this.ctx.fillStyle = bgColor;
    this.ctx.fillRect(0, 0, width, height);

    this.offscreenCtx.putImageData(this.imageData, 0, 0);

    const drawableWidth = width - this.leftMargin - this.rightMargin;
    const drawableHeight = height - this.topMargin - this.bottomMargin;
    this.ctx.drawImage(
      this.offscreenCanvas,
      0,
      0,
      this.offscreenCanvas.width,
      this.offscreenCanvas.height,
      this.leftMargin,
      this.topMargin,
      drawableWidth,
      drawableHeight
    );

    this.drawGrid(width, height);
  }

  private scrollLeft(): void {
    if (!this.imageData) return;
    const data = this.imageData.data;
    const w = this.imageData.width;
    const h = this.imageData.height;
    for (let y = 0; y < h; y++) {
      const rowStart = y * w * 4;
      for (let x = 0; x < w - 1; x++) {
        const destIdx = rowStart + x * 4;
        const srcIdx = rowStart + (x + 1) * 4;
        data[destIdx] = data[srcIdx];
        data[destIdx + 1] = data[srcIdx + 1];
        data[destIdx + 2] = data[srcIdx + 2];
        data[destIdx + 3] = data[srcIdx + 3];
      }
    }
  }

  private freqToYPosition(freq: number): number {
    const minLog = Math.log10(20);
    const maxLog = Math.log10(24000);
    const logFreq = Math.log10(Math.max(20, Math.min(24000, freq)));
    return 1 - (logFreq - minLog) / (maxLog - minLog);
  }

  private drawColumn(colors: number[]): void {
    if (!this.imageData) return;
    const data = this.imageData.data;
    const w = this.imageData.width;
    const h = this.imageData.height;
    const numPixels = Math.floor(colors.length / 3);
    const x = w - 1;
    const scaleY = numPixels / h;

    for (let y = 0; y < h; y++) {
      const srcY = Math.floor(y * scaleY);
      const srcIdx = Math.min(srcY, numPixels - 1) * 3;
      const idx = (y * w + x) * 4;
      data[idx] = colors[srcIdx] || 10;
      data[idx + 1] = colors[srcIdx + 1] || 15;
      data[idx + 2] = colors[srcIdx + 2] || 26;
      data[idx + 3] = 255;
    }
  }

  private drawGrid(width: number, height: number): void {
    const gridColor = cssVar(
      "--vtx-spectrogram-grid",
      "rgba(255,255,255,0.12)"
    );
    const textColor = cssVar("--vtx-waveform-text", "rgba(255,255,255,0.5)");
    const graphWidth = width - this.leftMargin - this.rightMargin;
    const graphHeight = height - this.topMargin - this.bottomMargin;

    this.ctx.strokeStyle = gridColor;
    this.ctx.lineWidth = 1;

    const gridFrequencies = [
      20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000,
    ];
    for (const freq of gridFrequencies) {
      const yPos = this.freqToYPosition(freq);
      const y = this.topMargin + yPos * graphHeight;
      this.ctx.beginPath();
      this.ctx.moveTo(this.leftMargin, y);
      this.ctx.lineTo(this.leftMargin + graphWidth, y);
      this.ctx.stroke();
    }

    for (let i = 0; i <= 16; i++) {
      const x = this.leftMargin + (graphWidth / 16) * i;
      this.ctx.beginPath();
      this.ctx.moveTo(x, this.topMargin);
      this.ctx.lineTo(x, this.topMargin + graphHeight);
      this.ctx.stroke();
    }

    this.ctx.fillStyle = textColor;
    this.ctx.font = "10px system-ui, sans-serif";
    this.ctx.textAlign = "right";
    this.ctx.textBaseline = "middle";

    const labelFreqs = [100, 500, 1000, 5000, 20000];
    const labelNames = ["100", "500", "1k", "5k", "20k"];
    for (let i = 0; i < labelFreqs.length; i++) {
      const yPos = this.freqToYPosition(labelFreqs[i]);
      const y = this.topMargin + yPos * graphHeight;
      this.ctx.fillText(labelNames[i], this.leftMargin - 4, y);
    }

    this.ctx.textAlign = "center";
    this.ctx.textBaseline = "top";
    const timeLabels = ["-2.5s", "-2s", "-1.5s", "-1s", "-0.5s", "0"];
    for (let i = 0; i < timeLabels.length; i++) {
      const x =
        this.leftMargin + (graphWidth / (timeLabels.length - 1)) * i;
      this.ctx.fillText(timeLabels[i], x, this.topMargin + graphHeight + 4);
    }
  }
}

// ---------------------------------------------------------------------------
// SpeechActivityRenderer
// ---------------------------------------------------------------------------

interface BufferedMetric {
  amplitude: number;
  zcr: number;
  centroid: number;
  speaking: boolean;
  voicedPending: boolean;
  whisperPending: boolean;
  transient: boolean;
  isLookbackSpeech: boolean;
  isWordBreak: boolean;
}

/** A segment-submission marker keyed by absolute frame index. */
interface SegmentMarker {
  /** Absolute frame index (value of totalFrames) at submission time. */
  frameIndex: number;
}

/** A slice of history-buffer data for one draw frame. */
interface VisibleSlice {
  amplitude: Float32Array;
  zcr: Float32Array;
  centroid: Float32Array;
  speaking: Uint8Array;
  lookback: Uint8Array;
  voicedPending: Uint8Array;
  whisperPending: Uint8Array;
  transient: Uint8Array;
  wordBreak: Uint8Array;
  /** Index in history arrays where this slice starts (before zero-padding). */
  startIndex: number;
}

/**
 * Multi-metric overlay showing speech detection algorithm components:
 * amplitude, ZCR, centroid as line graphs, plus speech state bar.
 *
 * Supports scrolling backward through history via mouse wheel or pointer drag.
 */
export class SpeechActivityRenderer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private animationId: number | null = null;
  private isActive = false;

  // ---------------------------------------------------------------------------
  // History buffers (append-only, grow up to maxHistoryFrames)
  // ---------------------------------------------------------------------------
  private histAmplitude: Float32Array;
  private histZcr: Float32Array;
  private histCentroid: Float32Array;
  private histSpeaking: Uint8Array;
  private histLookback: Uint8Array;
  private histVoicedPending: Uint8Array;
  private histWhisperPending: Uint8Array;
  private histTransient: Uint8Array;
  private histWordBreak: Uint8Array;

  /** How many frames have been written into the history arrays. */
  private totalFrames = 0;

  /** Maximum frames to keep in history before front-trimming. */
  private readonly maxHistoryFrames: number;

  /** Visible window width in frames. */
  private readonly bufferSize: number;

  // ---------------------------------------------------------------------------
  // Scroll state
  // ---------------------------------------------------------------------------
  /** Frames from the live head the view is anchored. 0 = live edge. */
  private scrollOffset = 0;

  // ---------------------------------------------------------------------------
  // Interaction state (sub-frame accumulator for smooth wheel/drag scrolling)
  // ---------------------------------------------------------------------------
  /** Fractional frame accumulator — shared by wheel and drag handlers in main.ts. */
  scrollAccum = 0;

  // ---------------------------------------------------------------------------
  // Delay buffer (preserved for lookback support)
  // ---------------------------------------------------------------------------
  private readonly delayBufferSize = 20;
  private delayBuffer: BufferedMetric[] = [];

  /** Segment-submission markers keyed by absolute frame index. */
  private segmentMarkers: SegmentMarker[] = [];

  // ---------------------------------------------------------------------------
  // Frame interval for dynamic x-axis labels
  // ---------------------------------------------------------------------------
  /** Expected milliseconds between visualization frames (default 16 ms). */
  frameIntervalMs = 16;

  private readonly leftMargin = 40;
  private readonly rightMargin = 8;
  private readonly topMargin = 8;
  private readonly bottomMargin = 20;

  private readonly minDb = -60;
  private readonly maxDb = 0;
  private readonly maxZcr = 0.5;
  private readonly maxCentroid = 8000;
  private readonly voicedThresholdDb = -40;
  private readonly whisperThresholdDb = -50;

  constructor(
    canvas: HTMLCanvasElement,
    bufferSize = 256,
    maxHistoryFrames = 108_000
  ) {
    this.canvas = canvas;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Could not get canvas 2D context");
    this.ctx = ctx;
    this.bufferSize = bufferSize;
    this.maxHistoryFrames = Math.max(maxHistoryFrames, bufferSize * 2);

    // Allocate history arrays at max capacity up front to avoid reallocation.
    this.histAmplitude = new Float32Array(this.maxHistoryFrames);
    this.histZcr = new Float32Array(this.maxHistoryFrames);
    this.histCentroid = new Float32Array(this.maxHistoryFrames);
    this.histSpeaking = new Uint8Array(this.maxHistoryFrames);
    this.histLookback = new Uint8Array(this.maxHistoryFrames);
    this.histVoicedPending = new Uint8Array(this.maxHistoryFrames);
    this.histWhisperPending = new Uint8Array(this.maxHistoryFrames);
    this.histTransient = new Uint8Array(this.maxHistoryFrames);
    this.histWordBreak = new Uint8Array(this.maxHistoryFrames);

    this.setupCanvas();
  }

  // ---------------------------------------------------------------------------
  // Setup
  // ---------------------------------------------------------------------------

  private setupCanvas(): void {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.scale(dpr, dpr);
  }

  // (Interaction handlers are wired externally in main.ts via the public scroll API.)

  // ---------------------------------------------------------------------------
  // Public scroll API
  // ---------------------------------------------------------------------------

  /**
   * Adjust scroll offset by `deltaFrames`. Positive moves into history;
   * negative moves toward the live edge. Clamped to valid range.
   * Triggers a redraw when the renderer is stopped (no active rAF loop).
   */
  scrollBy(deltaFrames: number): void {
    const maxOffset = Math.max(0, this.totalFrames - this.bufferSize);
    const prev = this.scrollOffset;
    this.scrollOffset = Math.max(
      0,
      Math.min(this.scrollOffset + deltaFrames, maxOffset)
    );
    if (this.scrollOffset !== prev && !this.isActive) {
      this.draw();
    }
  }

  /** Snap back to the live edge. Triggers a redraw when the renderer is stopped. */
  resetToLive(): void {
    const prev = this.scrollOffset;
    this.scrollOffset = 0;
    if (this.scrollOffset !== prev && !this.isActive) {
      this.draw();
    }
  }

  /** True when the view is pinned to the live (rightmost) edge. */
  get isLive(): boolean {
    return this.scrollOffset === 0;
  }

  /** The visible window width in frames (used by external scroll handlers). */
  get bufferFrames(): number {
    return this.bufferSize;
  }

  // ---------------------------------------------------------------------------
  // Data ingestion
  // ---------------------------------------------------------------------------

  pushMetrics(metrics: SpeechMetrics): void {
    const normalizedAmplitude = Math.max(
      0,
      Math.min(
        1,
        (metrics.amplitude_db - this.minDb) / (this.maxDb - this.minDb)
      )
    );
    const normalizedZcr = Math.min(1, metrics.zcr / this.maxZcr);
    const normalizedCentroid = Math.min(
      1,
      metrics.centroid_hz / this.maxCentroid
    );

    const buffered: BufferedMetric = {
      amplitude: normalizedAmplitude,
      zcr: normalizedZcr,
      centroid: normalizedCentroid,
      speaking: metrics.is_speaking,
      voicedPending: metrics.voiced_onset_pending,
      whisperPending: metrics.whisper_onset_pending,
      transient: metrics.is_transient,
      isLookbackSpeech: false,
      isWordBreak: metrics.is_word_break,
    };

    this.delayBuffer.push(buffered);

    if (this.delayBuffer.length > this.delayBufferSize) {
      const oldest = this.delayBuffer.shift()!;
      this.appendToHistory(oldest);
    }
  }

  private appendToHistory(metric: BufferedMetric): void {
    // If at cap, trim the front by one bufferSize chunk to amortize cost.
    if (this.totalFrames >= this.maxHistoryFrames) {
      this.trimHistory(this.bufferSize);
    }

    const i = this.totalFrames;
    this.histAmplitude[i] = metric.amplitude;
    this.histZcr[i] = metric.zcr;
    this.histCentroid[i] = metric.centroid;
    this.histSpeaking[i] = metric.speaking ? 1 : 0;
    this.histLookback[i] = metric.isLookbackSpeech ? 1 : 0;
    this.histVoicedPending[i] = metric.voicedPending ? 1 : 0;
    this.histWhisperPending[i] = metric.whisperPending ? 1 : 0;
    this.histTransient[i] = metric.transient ? 1 : 0;
    this.histWordBreak[i] = metric.isWordBreak ? 1 : 0;

    this.totalFrames += 1;
  }

  /**
   * Drop the oldest `count` frames from the history by shifting arrays left.
   * Segment marker frameIndices remain valid because they reference absolute
   * frame counts stored separately — but we need to subtract `count` from each
   * so they still point to the correct relative position after trimming.
   *
   * We keep a running `trimTotal` to avoid adjusting absolute indices directly:
   * instead, marker.frameIndex is always relative to the *current* history head.
   * After trimming, re-anchor all markers.
   */
  private trimHistory(count: number): void {
    const keep = this.totalFrames - count;
    if (keep <= 0) {
      this.totalFrames = 0;
      this.scrollOffset = 0;
      return;
    }

    this.histAmplitude.copyWithin(0, count, this.totalFrames);
    this.histZcr.copyWithin(0, count, this.totalFrames);
    this.histCentroid.copyWithin(0, count, this.totalFrames);
    this.histSpeaking.copyWithin(0, count, this.totalFrames);
    this.histLookback.copyWithin(0, count, this.totalFrames);
    this.histVoicedPending.copyWithin(0, count, this.totalFrames);
    this.histWhisperPending.copyWithin(0, count, this.totalFrames);
    this.histTransient.copyWithin(0, count, this.totalFrames);
    this.histWordBreak.copyWithin(0, count, this.totalFrames);

    this.totalFrames = keep;

    // Adjust scroll offset so the viewed position stays the same.
    this.scrollOffset = Math.max(0, this.scrollOffset - count);

    // Adjust segment markers; remove any that were trimmed away.
    this.segmentMarkers = this.segmentMarkers
      .map((m) => ({ frameIndex: m.frameIndex - count }))
      .filter((m) => m.frameIndex >= 0);
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  start(): void {
    if (this.isActive) return;
    this.isActive = true;
    this.animate();
  }

  stop(): void {
    this.isActive = false;
    if (this.animationId !== null) {
      cancelAnimationFrame(this.animationId);
      this.animationId = null;
    }
    // Redraw once so the LIVE indicator (which only shows while active) is erased.
    this.draw();
  }

  get active(): boolean {
    return this.isActive;
  }

  /**
   * Record a segment-submission marker at the current visualization position.
   * The absolute frame index is stored so the marker survives indefinitely,
   * rendering at the correct position regardless of scroll offset.
   */
  markSegmentSubmitted(): void {
    this.segmentMarkers.push({ frameIndex: this.totalFrames });
    // Keep no more than a reasonable maximum to prevent unbounded growth.
    if (this.segmentMarkers.length > this.maxHistoryFrames / this.bufferSize + 1) {
      this.segmentMarkers.shift();
    }
  }

  clear(): void {
    this.histAmplitude.fill(0);
    this.histZcr.fill(0);
    this.histCentroid.fill(0);
    this.histSpeaking.fill(0);
    this.histLookback.fill(0);
    this.histVoicedPending.fill(0);
    this.histWhisperPending.fill(0);
    this.histTransient.fill(0);
    this.histWordBreak.fill(0);
    this.totalFrames = 0;
    this.scrollOffset = 0;
    this.delayBuffer = [];
    this.segmentMarkers = [];
    this.drawIdle();
  }

  resize(): void {
    this.setupCanvas();
    this.drawIdle();
  }

  drawIdle(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    this.ctx.fillStyle = cssVar("--vtx-waveform-bg", "#1e293b");
    this.ctx.fillRect(0, 0, width, height);
    this.drawGrid(width, height);
  }

  // ---------------------------------------------------------------------------
  // Animation loop
  // ---------------------------------------------------------------------------

  private animate = (): void => {
    if (!this.isActive) return;
    this.draw();
    this.animationId = requestAnimationFrame(this.animate);
  };

  // ---------------------------------------------------------------------------
  // Visible-slice helper
  // ---------------------------------------------------------------------------

  /**
   * Return typed-array slices of exactly `bufferSize` frames representing the
   * currently visible window: from `startIndex` to `startIndex + bufferSize`.
   * If the window extends before the start of recorded history the left portion
   * is zero-padded (typed arrays default to zero so the slice is naturally
   * padded when `startIndex < 0`).
   */
  private getVisibleSlice(): VisibleSlice {
    const bs = this.bufferSize;
    // The rightmost visible frame is `totalFrames - 1 - scrollOffset`.
    // The leftmost visible frame is that minus (bufferSize - 1).
    const rightFrame = this.totalFrames - 1 - this.scrollOffset;
    const leftFrame = rightFrame - (bs - 1);
    const startIndex = Math.max(0, leftFrame);
    const endIndex = Math.max(0, rightFrame + 1); // exclusive

    const available = endIndex - startIndex; // may be < bufferSize
    const pad = bs - available; // frames to zero-pad on the left

    const amplitude = new Float32Array(bs);
    const zcr = new Float32Array(bs);
    const centroid = new Float32Array(bs);
    const speaking = new Uint8Array(bs);
    const lookback = new Uint8Array(bs);
    const voicedPending = new Uint8Array(bs);
    const whisperPending = new Uint8Array(bs);
    const transient = new Uint8Array(bs);
    const wordBreak = new Uint8Array(bs);

    if (available > 0) {
      amplitude.set(this.histAmplitude.subarray(startIndex, endIndex), pad);
      zcr.set(this.histZcr.subarray(startIndex, endIndex), pad);
      centroid.set(this.histCentroid.subarray(startIndex, endIndex), pad);
      speaking.set(this.histSpeaking.subarray(startIndex, endIndex), pad);
      lookback.set(this.histLookback.subarray(startIndex, endIndex), pad);
      voicedPending.set(this.histVoicedPending.subarray(startIndex, endIndex), pad);
      whisperPending.set(this.histWhisperPending.subarray(startIndex, endIndex), pad);
      transient.set(this.histTransient.subarray(startIndex, endIndex), pad);
      wordBreak.set(this.histWordBreak.subarray(startIndex, endIndex), pad);
    }

    return {
      amplitude, zcr, centroid, speaking, lookback,
      voicedPending, whisperPending, transient, wordBreak,
      startIndex,
    };
  }

  // ---------------------------------------------------------------------------
  // Draw
  // ---------------------------------------------------------------------------

  private draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    const area = this.getDrawableArea();

    const bgColor = cssVar("--vtx-waveform-bg", "#1e293b");
    this.ctx.fillStyle = bgColor;
    this.ctx.fillRect(0, 0, width, height);
    this.drawGrid(width, height);

    if (this.totalFrames === 0) return;

    const slice = this.getVisibleSlice();

    // Speech bar (top)
    const speechBarHeight = area.height * 0.08;
    this.drawSpeechBar(slice.speaking, slice.lookback, area);

    // Word break indicator bar (just below speaking bar)
    const wordBreakBarHeight = area.height * 0.08;
    const wordBreakArea = {
      x: area.x,
      y: area.y + speechBarHeight,
      width: area.width,
      height: wordBreakBarHeight,
    };
    this.drawWordBreakBars(slice.wordBreak, slice.speaking, wordBreakArea);

    // Metric lines and state markers use the area below both bars
    const barsHeight = speechBarHeight + wordBreakBarHeight;
    const metricsArea = {
      x: area.x,
      y: area.y + barsHeight,
      width: area.width,
      height: area.height - barsHeight,
    };

    // Metric lines
    this.drawMetricLine(
      slice.amplitude,
      metricsArea,
      cssVar("--vtx-metric-amplitude", "rgba(245,158,11,0.75)")
    );
    this.drawMetricLine(
      slice.zcr,
      metricsArea,
      cssVar("--vtx-metric-zcr", "rgba(6,182,212,0.75)")
    );
    this.drawMetricLine(
      slice.centroid,
      metricsArea,
      cssVar("--vtx-metric-centroid", "rgba(217,70,239,0.75)")
    );

    // State markers
    this.drawStateMarkers(
      slice.voicedPending,
      metricsArea,
      cssVar("--vtx-marker-voiced", "rgba(34,197,94,0.7)")
    );
    this.drawStateMarkers(
      slice.whisperPending,
      metricsArea,
      cssVar("--vtx-marker-whisper", "rgba(59,130,246,0.7)")
    );
    this.drawStateMarkers(
      slice.transient,
      metricsArea,
      cssVar("--vtx-marker-transient", "rgba(239,68,68,0.7)")
    );

    // Segment-submission markers (drawn on top)
    this.drawSegmentMarkers(area);

    // Scroll indicator (topmost layer)
    this.drawScrollIndicator(area);
  }

  private getDrawableArea() {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    return {
      x: this.leftMargin,
      y: this.topMargin,
      width: width - this.leftMargin - this.rightMargin,
      height: height - this.topMargin - this.bottomMargin,
    };
  }

  // ---------------------------------------------------------------------------
  // Sub-draw methods (all operate on full-bufferSize slices)
  // ---------------------------------------------------------------------------

  private drawSpeechBar(
    speaking: Uint8Array,
    lookback: Uint8Array,
    area: { x: number; y: number; width: number; height: number }
  ): void {
    const barHeight = area.height * 0.08;
    const bs = this.bufferSize;
    const confirmed = cssVar("--vtx-speech-confirmed", "rgba(34,197,94,0.5)");
    const lookbackColor = cssVar("--vtx-speech-lookback", "rgba(59,130,246,0.7)");

    // Lookback regions
    this.ctx.fillStyle = lookbackColor;
    let inLookback = false;
    let startX = 0;
    for (let i = 0; i <= lookback.length; i++) {
      const isLb = i < lookback.length && lookback[i] === 1;
      const x = area.x + (i / bs) * area.width;
      if (isLb && !inLookback) {
        inLookback = true;
        startX = x;
      } else if (!isLb && inLookback) {
        inLookback = false;
        this.ctx.fillRect(startX, area.y, x - startX, barHeight);
      }
    }

    // Confirmed speech
    this.ctx.fillStyle = confirmed;
    let inSpeech = false;
    for (let i = 0; i <= speaking.length; i++) {
      const isSp =
        i < speaking.length &&
        speaking[i] === 1 &&
        !(i < lookback.length && lookback[i] === 1);
      const x = area.x + (i / bs) * area.width;
      if (isSp && !inSpeech) {
        inSpeech = true;
        startX = x;
      } else if (!isSp && inSpeech) {
        inSpeech = false;
        this.ctx.fillRect(startX, area.y, x - startX, barHeight);
      }
    }
  }

  private drawWordBreakBars(
    wordBreaks: Uint8Array,
    speaking: Uint8Array,
    area: { x: number; y: number; width: number; height: number }
  ): void {
    const barHeight = area.height;
    const bs = this.bufferSize;
    this.ctx.strokeStyle = cssVar(
      "--vtx-speech-word-break",
      "rgba(249,115,22,0.85)"
    );
    this.ctx.lineWidth = 2;

    for (let i = 0; i < wordBreaks.length; i++) {
      if (wordBreaks[i] === 1 && i < speaking.length && speaking[i] === 1) {
        const x = area.x + (i / bs) * area.width;
        this.ctx.beginPath();
        this.ctx.moveTo(x, area.y);
        this.ctx.lineTo(x, area.y + barHeight);
        this.ctx.stroke();
      }
    }
  }

  private drawMetricLine(
    values: Float32Array,
    area: { x: number; y: number; width: number; height: number },
    color: string
  ): void {
    if (values.length === 0) return;
    this.ctx.beginPath();
    this.ctx.strokeStyle = color;
    this.ctx.lineWidth = 1;

    const bs = this.bufferSize;
    for (let i = 0; i < values.length; i++) {
      const x = area.x + (i / bs) * area.width;
      const y = area.y + area.height - values[i] * area.height;
      if (i === 0) this.ctx.moveTo(x, y);
      else this.ctx.lineTo(x, y);
    }
    this.ctx.stroke();
  }

  private drawStateMarkers(
    states: Uint8Array,
    area: { x: number; y: number; width: number; height: number },
    color: string
  ): void {
    if (states.length === 0) return;
    this.ctx.fillStyle = color;
    const bs = this.bufferSize;
    for (let i = 0; i < states.length; i++) {
      if (states[i] === 1) {
        const x = area.x + (i / bs) * area.width;
        const y = area.y + area.height - 4;
        this.ctx.beginPath();
        this.ctx.arc(x, y, 2, 0, Math.PI * 2);
        this.ctx.fill();
      }
    }
  }

  private drawSegmentMarkers(
    area: { x: number; y: number; width: number; height: number }
  ): void {
    if (this.segmentMarkers.length === 0) return;

    const color = cssVar("--vtx-segment-marker", "rgba(255,255,255,0.85)");
    const bs = this.bufferSize;

    // The rightmost visible absolute frame index.
    const rightFrame = this.totalFrames - 1 - this.scrollOffset;

    for (const marker of this.segmentMarkers) {
      // How many frames ago relative to the right edge is this marker?
      const slotOffset = rightFrame - marker.frameIndex;

      // Skip if outside the visible window.
      if (slotOffset < 0 || slotOffset >= bs) continue;

      // Convert to canvas X: rightmost = area.x + area.width, oldest = area.x.
      const x = area.x + ((bs - 1 - slotOffset) / (bs - 1)) * area.width;

      // Full-height vertical dashed line
      this.ctx.save();
      this.ctx.strokeStyle = color;
      this.ctx.lineWidth = 1.5;
      this.ctx.setLineDash([4, 3]);
      this.ctx.beginPath();
      this.ctx.moveTo(x, area.y);
      this.ctx.lineTo(x, area.y + area.height);
      this.ctx.stroke();
      this.ctx.setLineDash([]);
      this.ctx.restore();

      // Small downward-pointing triangle at the top
      const triSize = 5;
      this.ctx.save();
      this.ctx.fillStyle = color;
      this.ctx.beginPath();
      this.ctx.moveTo(x - triSize, area.y);
      this.ctx.lineTo(x + triSize, area.y);
      this.ctx.lineTo(x, area.y + triSize * 1.4);
      this.ctx.closePath();
      this.ctx.fill();
      this.ctx.restore();
    }
  }

  private drawScrollIndicator(
    area: { x: number; y: number; width: number; height: number }
  ): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;

    const textColor = cssVar("--vtx-waveform-text", "rgba(255,255,255,0.5)");
    this.ctx.font = "bold 10px system-ui, sans-serif";
    this.ctx.textBaseline = "top";

    if (this.scrollOffset === 0 && this.isActive) {
      // "● LIVE" pill in green — only while the renderer is actively receiving data
      const label = "\u25CF LIVE";
      this.ctx.fillStyle = "rgba(34,197,94,0.9)";
      const textWidth = this.ctx.measureText(label).width;
      const padX = 5;
      const padY = 3;
      const pillW = textWidth + padX * 2;
      const pillH = 14;
      const pillX = width - this.rightMargin - pillW - 2;
      const pillY = this.topMargin + 2;
      this.ctx.fillRect(pillX - padX, pillY - padY, pillW + padX, pillH);
      this.ctx.fillStyle = "#fff";
      this.ctx.textAlign = "left";
      this.ctx.fillText(label, pillX, pillY);
    } else if (this.scrollOffset > 0) {
      // "◀ -Xs" label in muted color
      const seconds = (this.scrollOffset * this.frameIntervalMs / 1000).toFixed(1);
      const label = `\u25C4 -${seconds}s`;
      this.ctx.fillStyle = textColor;
      this.ctx.textAlign = "right";
      this.ctx.fillText(label, width - this.rightMargin - 2, this.topMargin + 2);
    }
  }

  private drawGrid(width: number, height: number): void {
    const gridColor = cssVar("--vtx-waveform-grid", "rgba(255,255,255,0.08)");
    const textColor = cssVar("--vtx-waveform-text", "rgba(255,255,255,0.5)");
    const graphWidth = width - this.leftMargin - this.rightMargin;
    const graphHeight = height - this.topMargin - this.bottomMargin;

    this.ctx.strokeStyle = gridColor;
    this.ctx.lineWidth = 1;

    for (let i = 0; i <= 5; i++) {
      const y = this.topMargin + (graphHeight / 5) * i;
      this.ctx.beginPath();
      this.ctx.moveTo(this.leftMargin, y);
      this.ctx.lineTo(this.leftMargin + graphWidth, y);
      this.ctx.stroke();
    }
    for (let i = 0; i <= 16; i++) {
      const x = this.leftMargin + (graphWidth / 16) * i;
      this.ctx.beginPath();
      this.ctx.moveTo(x, this.topMargin);
      this.ctx.lineTo(x, this.topMargin + graphHeight);
      this.ctx.stroke();
    }

    // Threshold lines
    const thresholdColor = cssVar(
      "--vtx-threshold-line",
      "rgba(255,255,255,0.15)"
    );
    this.ctx.strokeStyle = thresholdColor;
    this.ctx.lineWidth = 1.5;

    const voicedY =
      this.topMargin +
      graphHeight -
      ((this.voicedThresholdDb - this.minDb) / (this.maxDb - this.minDb)) *
        graphHeight;
    this.ctx.beginPath();
    this.ctx.moveTo(this.leftMargin, voicedY);
    this.ctx.lineTo(this.leftMargin + graphWidth, voicedY);
    this.ctx.stroke();

    const whisperY =
      this.topMargin +
      graphHeight -
      ((this.whisperThresholdDb - this.minDb) / (this.maxDb - this.minDb)) *
        graphHeight;
    this.ctx.beginPath();
    this.ctx.moveTo(this.leftMargin, whisperY);
    this.ctx.lineTo(this.leftMargin + graphWidth, whisperY);
    this.ctx.stroke();

    // Y-axis labels
    this.ctx.fillStyle = textColor;
    this.ctx.font = "9px system-ui, sans-serif";
    this.ctx.textAlign = "right";
    this.ctx.textBaseline = "middle";
    for (const db of [0, -20, -40, -50, -60]) {
      const normalizedY = (db - this.minDb) / (this.maxDb - this.minDb);
      const y = this.topMargin + graphHeight - normalizedY * graphHeight;
      const label = db === -40 ? "-40V" : db === -50 ? "-50W" : `${db}`;
      this.ctx.fillText(label, this.leftMargin - 3, y);
    }

    // X-axis labels — computed dynamically from frame interval and scroll offset
    this.ctx.textAlign = "center";
    this.ctx.textBaseline = "top";
    const numLabels = 6;
    for (let i = 0; i < numLabels; i++) {
      // Column index within the bufferSize window, evenly spaced.
      const colIndex = Math.round(((this.bufferSize - 1) / (numLabels - 1)) * i);
      // Time relative to the live head (negative = in the past).
      const t =
        -(this.bufferSize - 1 - colIndex + this.scrollOffset) *
        this.frameIntervalMs /
        1000;
      const label = `${t.toFixed(1)}s`;
      const x = this.leftMargin + (graphWidth / (numLabels - 1)) * i;
      this.ctx.fillText(label, x, this.topMargin + graphHeight + 4);
    }
  }
}

// ---------------------------------------------------------------------------
// MiniWaveformRenderer
// ---------------------------------------------------------------------------

/**
 * Stylized mini waveform for compact display (e.g., header bars).
 * Applies center-attenuated amplitude with cosine falloff and Catmull-Rom
 * spline smoothing.
 */
export class MiniWaveformRenderer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private animationId: number | null = null;
  private ringBuffer: RingBuffer;
  private isActive = false;

  constructor(canvas: HTMLCanvasElement, bufferSize = 512) {
    this.canvas = canvas;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Could not get canvas 2D context");
    this.ctx = ctx;
    this.ringBuffer = new RingBuffer(bufferSize);
    this.setupCanvas();
  }

  private setupCanvas(): void {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.scale(dpr, dpr);
  }

  pushSamples(samples: number[]): void {
    this.ringBuffer.push(samples);
  }

  start(): void {
    if (this.isActive) return;
    this.isActive = true;
    this.animate();
  }

  stop(): void {
    this.isActive = false;
    if (this.animationId !== null) {
      cancelAnimationFrame(this.animationId);
      this.animationId = null;
    }
  }

  get active(): boolean {
    return this.isActive;
  }

  clear(): void {
    this.ringBuffer.clear();
    this.drawIdle();
  }

  resize(): void {
    this.setupCanvas();
    this.drawIdle();
  }

  drawIdle(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    this.ctx.clearRect(0, 0, width, height);
    const centerY = height / 2;
    this.ctx.strokeStyle = cssVar("--vtx-waveform-line", "#475569");
    this.ctx.lineWidth = 1;
    this.ctx.beginPath();
    this.ctx.moveTo(0, centerY);
    this.ctx.lineTo(width, centerY);
    this.ctx.stroke();
  }

  private animate = (): void => {
    if (!this.isActive) return;
    this.draw();
    this.animationId = requestAnimationFrame(this.animate);
  };

  private attenuation(t: number): number {
    const distFromCenter = Math.abs(t - 0.5) * 2;
    return Math.cos((distFromCenter * Math.PI) / 2) * 2;
  }

  private draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    const samples = this.ringBuffer.getSamples();

    this.ctx.clearRect(0, 0, width, height);

    if (samples.length === 0) {
      this.drawIdle();
      return;
    }

    const waveformColor = cssVar("--vtx-waveform-color", "#3b82f6");
    const glowColor = cssVar(
      "--vtx-waveform-glow",
      "rgba(59, 130, 246, 0.5)"
    );
    const centerY = height / 2;
    const maxAmplitude = height / 2 - 2;
    const pointCount = samples.length;

    // Find peak for scaling
    let peakAtt = 0;
    for (let i = 0; i < pointCount; i++) {
      const t = i / (pointCount - 1);
      peakAtt = Math.max(peakAtt, Math.abs(samples[i] || 0) * this.attenuation(t));
    }
    const scale = peakAtt > 1 ? 1 / peakAtt : 1;

    // Build points
    const points: { x: number; y: number }[] = [];
    for (let i = 0; i < pointCount; i++) {
      const t = i / (pointCount - 1);
      const att = this.attenuation(t);
      const sample = Math.max(-1, Math.min(1, samples[i] || 0));
      points.push({
        x: t * width,
        y: centerY - sample * maxAmplitude * att * scale,
      });
    }

    // Catmull-Rom spline path
    this.ctx.beginPath();
    if (points.length > 0) {
      this.ctx.moveTo(points[0].x, points[0].y);
      for (let i = 0; i < points.length - 1; i++) {
        const p0 = points[Math.max(0, i - 1)];
        const p1 = points[i];
        const p2 = points[Math.min(points.length - 1, i + 1)];
        const p3 = points[Math.min(points.length - 1, i + 2)];
        const cp1x = p1.x + (p2.x - p0.x) / 6;
        const cp1y = p1.y + (p2.y - p0.y) / 6;
        const cp2x = p2.x - (p3.x - p1.x) / 6;
        const cp2y = p2.y - (p3.y - p1.y) / 6;
        this.ctx.bezierCurveTo(cp1x, cp1y, cp2x, cp2y, p2.x, p2.y);
      }
    }

    // Glow
    this.ctx.save();
    this.ctx.strokeStyle = glowColor;
    this.ctx.lineWidth = 4;
    this.ctx.filter = "blur(3px)";
    this.ctx.stroke();
    this.ctx.restore();

    // Main line
    this.ctx.strokeStyle = waveformColor;
    this.ctx.lineWidth = 1.5;
    this.ctx.stroke();
  }
}
