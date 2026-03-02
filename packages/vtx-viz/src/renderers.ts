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

/**
 * Multi-metric overlay showing speech detection algorithm components:
 * amplitude, ZCR, centroid as line graphs, plus speech state bar.
 */
export class SpeechActivityRenderer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private animationId: number | null = null;
  private isActive = false;

  private amplitudeBuffer: Float32Array;
  private zcrBuffer: Float32Array;
  private centroidBuffer: Float32Array;
  private speakingBuffer: Uint8Array;
  private lookbackSpeechBuffer: Uint8Array;
  private voicedPendingBuffer: Uint8Array;
  private whisperPendingBuffer: Uint8Array;
  private transientBuffer: Uint8Array;
  private wordBreakBuffer: Uint8Array;

  private bufferSize: number;
  private writeIndex = 0;
  private filled = false;

  private readonly delayBufferSize = 20;
  private delayBuffer: BufferedMetric[] = [];

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

  constructor(canvas: HTMLCanvasElement, bufferSize = 256) {
    this.canvas = canvas;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Could not get canvas 2D context");
    this.ctx = ctx;
    this.bufferSize = bufferSize;

    this.amplitudeBuffer = new Float32Array(bufferSize);
    this.zcrBuffer = new Float32Array(bufferSize);
    this.centroidBuffer = new Float32Array(bufferSize);
    this.speakingBuffer = new Uint8Array(bufferSize);
    this.lookbackSpeechBuffer = new Uint8Array(bufferSize);
    this.voicedPendingBuffer = new Uint8Array(bufferSize);
    this.whisperPendingBuffer = new Uint8Array(bufferSize);
    this.transientBuffer = new Uint8Array(bufferSize);
    this.wordBreakBuffer = new Uint8Array(bufferSize);

    this.setupCanvas();
  }

  private setupCanvas(): void {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.scale(dpr, dpr);
  }

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
      this.transferToRingBuffer(oldest);
    }
  }

  private transferToRingBuffer(metric: BufferedMetric): void {
    this.amplitudeBuffer[this.writeIndex] = metric.amplitude;
    this.zcrBuffer[this.writeIndex] = metric.zcr;
    this.centroidBuffer[this.writeIndex] = metric.centroid;
    this.speakingBuffer[this.writeIndex] = metric.speaking ? 1 : 0;
    this.lookbackSpeechBuffer[this.writeIndex] = metric.isLookbackSpeech
      ? 1
      : 0;
    this.voicedPendingBuffer[this.writeIndex] = metric.voicedPending ? 1 : 0;
    this.whisperPendingBuffer[this.writeIndex] = metric.whisperPending ? 1 : 0;
    this.transientBuffer[this.writeIndex] = metric.transient ? 1 : 0;
    this.wordBreakBuffer[this.writeIndex] = metric.isWordBreak ? 1 : 0;

    this.writeIndex = (this.writeIndex + 1) % this.bufferSize;
    if (this.writeIndex === 0) this.filled = true;
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
    this.amplitudeBuffer.fill(0);
    this.zcrBuffer.fill(0);
    this.centroidBuffer.fill(0);
    this.speakingBuffer.fill(0);
    this.lookbackSpeechBuffer.fill(0);
    this.voicedPendingBuffer.fill(0);
    this.whisperPendingBuffer.fill(0);
    this.transientBuffer.fill(0);
    this.wordBreakBuffer.fill(0);
    this.delayBuffer = [];
    this.writeIndex = 0;
    this.filled = false;
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

  private getSamplesInOrder<T extends Float32Array | Uint8Array>(buffer: T): T {
    if (!this.filled) return buffer.slice(0, this.writeIndex) as T;
    const result = new (buffer.constructor as any)(buffer.length);
    result.set(buffer.slice(this.writeIndex), 0);
    result.set(buffer.slice(0, this.writeIndex), buffer.length - this.writeIndex);
    return result;
  }

  private draw(): void {
    const dpr = window.devicePixelRatio || 1;
    const width = this.canvas.width / dpr;
    const height = this.canvas.height / dpr;
    const area = this.getDrawableArea();

    const bgColor = cssVar("--vtx-waveform-bg", "#1e293b");
    this.ctx.fillStyle = bgColor;
    this.ctx.fillRect(0, 0, width, height);
    this.drawGrid(width, height);

    const amplitudes = this.getSamplesInOrder(this.amplitudeBuffer);
    const zcrs = this.getSamplesInOrder(this.zcrBuffer);
    const centroids = this.getSamplesInOrder(this.centroidBuffer);
    const speaking = this.getSamplesInOrder(this.speakingBuffer);
    const lookback = this.getSamplesInOrder(this.lookbackSpeechBuffer);
    const wordBreaks = this.getSamplesInOrder(this.wordBreakBuffer);
    const voiced = this.getSamplesInOrder(this.voicedPendingBuffer);
    const whisper = this.getSamplesInOrder(this.whisperPendingBuffer);
    const transients = this.getSamplesInOrder(this.transientBuffer);

    if (amplitudes.length === 0) return;

    // Speech bar
    this.drawSpeechBar(speaking, lookback, area);
    this.drawWordBreakBars(wordBreaks, speaking, area);

    // Metric lines
    this.drawMetricLine(
      amplitudes,
      area,
      cssVar("--vtx-metric-amplitude", "rgba(245,158,11,0.75)")
    );
    this.drawMetricLine(
      zcrs,
      area,
      cssVar("--vtx-metric-zcr", "rgba(6,182,212,0.75)")
    );
    this.drawMetricLine(
      centroids,
      area,
      cssVar("--vtx-metric-centroid", "rgba(217,70,239,0.75)")
    );

    // State markers
    this.drawStateMarkers(
      voiced,
      area,
      cssVar("--vtx-marker-voiced", "rgba(34,197,94,0.7)")
    );
    this.drawStateMarkers(
      whisper,
      area,
      cssVar("--vtx-marker-whisper", "rgba(59,130,246,0.7)")
    );
    this.drawStateMarkers(
      transients,
      area,
      cssVar("--vtx-marker-transient", "rgba(239,68,68,0.7)")
    );
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

  private drawSpeechBar(
    speaking: Uint8Array,
    lookback: Uint8Array,
    area: { x: number; y: number; width: number; height: number }
  ): void {
    const barHeight = area.height * 0.15;
    const offset = this.bufferSize - speaking.length;
    const confirmed = cssVar(
      "--vtx-speech-confirmed",
      "rgba(34,197,94,0.5)"
    );
    const lookbackColor = cssVar(
      "--vtx-speech-lookback",
      "rgba(59,130,246,0.7)"
    );

    // Lookback regions
    this.ctx.fillStyle = lookbackColor;
    let inLookback = false;
    let startX = 0;
    for (let i = 0; i <= lookback.length; i++) {
      const isLb = i < lookback.length && lookback[i] === 1;
      const x = area.x + ((offset + i) / this.bufferSize) * area.width;
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
      const x = area.x + ((offset + i) / this.bufferSize) * area.width;
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
    const barHeight = area.height * 0.15;
    const offset = this.bufferSize - wordBreaks.length;
    this.ctx.strokeStyle = cssVar(
      "--vtx-speech-word-break",
      "rgba(249,115,22,0.85)"
    );
    this.ctx.lineWidth = 2;

    for (let i = 0; i < wordBreaks.length; i++) {
      if (
        wordBreaks[i] === 1 &&
        i < speaking.length &&
        speaking[i] === 1
      ) {
        const x = area.x + ((offset + i) / this.bufferSize) * area.width;
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

    for (let i = 0; i < values.length; i++) {
      const x =
        area.x +
        ((this.bufferSize - values.length + i) / this.bufferSize) * area.width;
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
    for (let i = 0; i < states.length; i++) {
      if (states[i] === 1) {
        const x =
          area.x +
          ((this.bufferSize - states.length + i) / this.bufferSize) *
            area.width;
        const y = area.y + area.height - 4;
        this.ctx.beginPath();
        this.ctx.arc(x, y, 2, 0, Math.PI * 2);
        this.ctx.fill();
      }
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
      const normalizedY =
        (db - this.minDb) / (this.maxDb - this.minDb);
      const y = this.topMargin + graphHeight - normalizedY * graphHeight;
      const label =
        db === -40 ? "-40V" : db === -50 ? "-50W" : `${db}`;
      this.ctx.fillText(label, this.leftMargin - 3, y);
    }

    // X-axis labels
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
