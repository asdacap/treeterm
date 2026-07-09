/**
 * E2E Tests for the terminal refresh button.
 *
 * Refresh disposes the xterm engine and re-attaches to the still-running daemon PTY. Disposing
 * a WebGL-backed xterm runs `WebglAddon`'s cleanup, which reaches into xterm's core internals —
 * so a `@xterm/addon-webgl` built against a different `@xterm/xterm` major throws there and
 * leaves the screen blank until a second click. Only a real GPU context reproduces it: the unit
 * suite mocks `WebglAddon`, and jsdom has no WebGL2, so the addon falls back to the DOM renderer
 * and its cleanup never runs. This test is the only guard against that class of regression.
 */

import { test, expect } from '@playwright/test'
import {
  launchApp,
  closeApp,
  killDaemon,
  waitForTerminalReady,
  cleanupTestData,
  getTerminalText
} from './helpers'

test.describe('Terminal refresh', () => {
  test.beforeEach(async () => {
    const { resetTestSocketPath } = await import('./helpers')
    resetTestSocketPath()
    killDaemon()
    cleanupTestData()
  })

  test.afterEach(async () => {
    killDaemon()
    cleanupTestData()
  })

  test('a single refresh click re-renders a live terminal', async () => {
    const { app, window } = await launchApp()
    await waitForTerminalReady(window)

    // Anything thrown while disposing the engine surfaces here.
    const pageErrors: Error[] = []
    window.on('pageerror', (error) => pageErrors.push(error))

    await window.keyboard.type('echo BEFORE_REFRESH')
    await window.keyboard.press('Enter')
    await expect.poll(() => getTerminalText(window)).toContain('BEFORE_REFRESH')

    await window.locator('button[title="Refresh stream"]').first().click()

    // The GPU renderer is torn down and rebuilt; the canvas must come back on the first click.
    await waitForTerminalReady(window)
    expect(pageErrors.map((error) => error.message)).toEqual([])

    // The rebuilt engine must be wired to the same PTY, not a dead one.
    await window.keyboard.type('echo AFTER_REFRESH')
    await window.keyboard.press('Enter')
    await expect.poll(() => getTerminalText(window)).toContain('AFTER_REFRESH')

    await closeApp(app)
  })
})
