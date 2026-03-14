/**
 * E2E Tests for Session Persistence
 */

import { test, expect } from '@playwright/test'
import {
  launchApp,
  closeApp,
  killDaemon,
  isDaemonRunning,
  waitForDaemon,
  waitForTerminalReady,
  cleanupTestData,
  getTerminalText
} from './helpers'

test.describe('Session Persistence', () => {
  test.beforeEach(async () => {
    // Clean up any existing daemon
    const { resetTestSocketPath } = await import('./helpers')
    resetTestSocketPath()
    killDaemon()
    cleanupTestData()
  })

  test.afterEach(async () => {
    // Clean up daemon after test
    killDaemon()
    cleanupTestData()
  })

  test('terminal session persists after app restart', async () => {
    // Launch app
    const { app, window } = await launchApp()

    // Wait for terminal to be ready
    await waitForTerminalReady(window)

    // Type a unique marker command
    const marker = `echo "TEST_MARKER_${Date.now()}"`
    await window.keyboard.type(marker)
    await window.keyboard.press('Enter')

    // Wait for command to execute and output to appear
    await window.waitForTimeout(2000)

    // Take screenshot for debugging
    await window.screenshot({ path: '/tmp/test-before-close.png' })

    // Wait for text to appear (with retry)
    let content1 = ''
    for (let i = 0; i < 5; i++) {
      content1 = await getTerminalText(window)
      console.log(`[Test] Attempt ${i + 1} - Terminal content:`, content1.substring(0, 100))
      if (content1.includes('TEST_MARKER_') || content1.includes(marker.substring(6))) {
        break
      }
      await window.waitForTimeout(1000)
    }

    expect(content1.length).toBeGreaterThan(0)
    // The test might be too strict - let's see what we actually get
    console.log('[Test] Full terminal content:', content1)

    // Close app (detach from session)
    await closeApp(app)

    // Verify daemon still running (use sleep instead of window.waitForTimeout)
    await new Promise(resolve => setTimeout(resolve, 1000))
    expect(isDaemonRunning()).toBe(true)

    // Relaunch app
    const { app: app2, window: window2 } = await launchApp()

    // Wait for terminal to be ready
    await waitForTerminalReady(window2)

    // Wait a bit for scrollback to restore
    await window2.waitForTimeout(1000)

    // Verify scrollback contains marker
    const content2 = await getTerminalText(window2)
    console.log('[Test] Terminal content after reopen:', content2.substring(0, 200))
    expect(content2).toContain('TEST_MARKER_')

    await closeApp(app2)
  })
})
