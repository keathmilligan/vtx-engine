/** Ring buffer for storing waveform samples with fixed capacity. */
export class RingBuffer {
  private buffer: Float32Array;
  private writeIndex: number = 0;
  private filled: boolean = false;

  constructor(capacity: number) {
    this.buffer = new Float32Array(capacity);
  }

  /** Push new samples into the buffer. */
  push(samples: number[]): void {
    for (const sample of samples) {
      this.buffer[this.writeIndex] = sample;
      this.writeIndex = (this.writeIndex + 1) % this.buffer.length;
      if (this.writeIndex === 0) {
        this.filled = true;
      }
    }
  }

  /** Get samples in chronological order (oldest to newest). */
  getSamples(): Float32Array {
    if (!this.filled) {
      return this.buffer.slice(0, this.writeIndex);
    }
    const result = new Float32Array(this.buffer.length);
    const secondPart = this.buffer.slice(this.writeIndex);
    const firstPart = this.buffer.slice(0, this.writeIndex);
    result.set(secondPart, 0);
    result.set(firstPart, secondPart.length);
    return result;
  }

  /** Clear all samples. */
  clear(): void {
    this.buffer.fill(0);
    this.writeIndex = 0;
    this.filled = false;
  }

  /** Number of samples currently stored. */
  get length(): number {
    return this.filled ? this.buffer.length : this.writeIndex;
  }
}
