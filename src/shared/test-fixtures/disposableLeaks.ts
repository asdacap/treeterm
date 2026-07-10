import { afterEach, beforeEach, expect } from 'vitest'
import {
  clearDisposableTracker,
  createLeakTrackingDisposableTracker,
  setDisposableTracker,
} from '../lifecycle'

/**
 * Fail the current test file if any `IDisposable` created during a test is still alive
 * when that test ends.
 *
 * Call once at the top of a `describe`. This is what turns "did we leak a subscription?"
 * from a code-review question into a build failure — see AGENTS.md, "For each change
 * make test".
 *
 * Opt-in per file rather than global: plenty of suites construct intentionally
 * long-lived stores, and a global assertion would flag those as leaks.
 */
export function expectNoDisposableLeaks(): void {
  let tracker = createLeakTrackingDisposableTracker()

  beforeEach(() => {
    tracker = createLeakTrackingDisposableTracker()
    setDisposableTracker(tracker)
  })

  afterEach(() => {
    const leaked = tracker.getLiveStacks()
    // Uninstall before asserting so a failure here cannot leak into the next test.
    clearDisposableTracker()
    expect(leaked, `${String(leaked.length)} disposable(s) were never disposed:\n${leaked.join('\n\n')}`).toEqual([])
  })
}
