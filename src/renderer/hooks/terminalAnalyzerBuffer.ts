export type AnalyzerResult = { state: string; reason: string }

export type BufferCheckResult =
  | { action: 'skip' }
  | { action: 'reuse'; result: AnalyzerResult }
  | { action: 'analyze' }

export class TerminalAnalyzerBuffer {
  private inFlightBuffer: string | null = null
  private lastAnalyzedBuffer: string | null = null
  private lastResult: AnalyzerResult | null = null

  check(buffer: string): BufferCheckResult {
    if (buffer === this.inFlightBuffer) {
      return { action: 'skip' }
    }
    if (buffer === this.lastAnalyzedBuffer && this.lastResult !== null) {
      return { action: 'reuse', result: this.lastResult }
    }
    return { action: 'analyze' }
  }

  setInFlight(buffer: string): void {
    this.inFlightBuffer = buffer
  }

  clearInFlight(): void {
    this.inFlightBuffer = null
  }

  setResult(buffer: string, result: AnalyzerResult): void {
    this.lastAnalyzedBuffer = buffer
    this.lastResult = result
    this.inFlightBuffer = null
  }

  reset(): void {
    this.inFlightBuffer = null
    this.lastAnalyzedBuffer = null
    this.lastResult = null
  }
}
