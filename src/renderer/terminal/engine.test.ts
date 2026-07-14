import { describe, it, expect } from 'vitest'
import { snapshotViewport } from './engine'
import type { TerminalEngine } from './engine'

/** Build a fake `{ raw, rows }` whose active buffer holds `lines`. */
function fakeEngine(lines: string[], rows: number): Pick<TerminalEngine, 'raw' | 'rows'> {
  return {
    rows,
    raw: {
      buffer: {
        active: {
          length: lines.length,
          getLine: (y: number) =>
            y >= 0 && y < lines.length
              ? { translateToString: () => lines[y] ?? '' }
              : undefined,
        },
      },
    },
  }
}

describe('snapshotViewport', () => {
  it('returns the tail `rows` lines when the buffer exceeds the viewport', () => {
    const lines = ['scrollback 0', 'scrollback 1', 'row a', 'row b', 'row c']
    const snapshot = snapshotViewport(fakeEngine(lines, 3))
    expect(snapshot).toBe('row a\nrow b\nrow c')
  })

  it('returns the whole buffer when it is shorter than the viewport', () => {
    const snapshot = snapshotViewport(fakeEngine(['only line'], 24))
    expect(snapshot).toBe('only line')
  })

  it('returns an empty string for an empty buffer', () => {
    expect(snapshotViewport(fakeEngine([], 24))).toBe('')
  })

  it('substitutes an empty string for a missing line', () => {
    // length claims 2 lines but getLine(1) is absent — must not throw.
    const engine: Pick<TerminalEngine, 'raw' | 'rows'> = {
      rows: 24,
      raw: {
        buffer: {
          active: {
            length: 2,
            getLine: (y: number) =>
              y === 0 ? { translateToString: () => 'present' } : undefined,
          },
        },
      },
    }
    expect(snapshotViewport(engine)).toBe('present\n')
  })
})
