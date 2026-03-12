/**
 * E2E Tests for Daemon Lifecycle
 */

import { test, expect } from '@playwright/test'
import {
  launchApp,
  closeApp,
  killDaemon,
  isDaemonRunning,
  getDaemonPid,
  waitForDaemon,
  waitForTerminalReady,
  cleanupTestData
} from './helpers'

test.describe('Daemon Lifecycle', () => {
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

  test('daemon auto-starts on first app launch', async () => {
    // Ensure no daemon running
    expect(isDaemonRunning()).toBe(false)

    const { app, window } = await launchApp()

    // Wait for daemon to start
    await waitForDaemon(10000)

    // Daemon should be running
    expect(isDaemonRunning()).toBe(true)

    const pid = getDaemonPid()
    expect(pid).not.toBeNull()
    console.log('[Test] Daemon PID:', pid)

    await closeApp(app)
  })

  test('daemon persists after all apps close', async () => {
    const { app, window } = await launchApp()
    await waitForTerminalReady(window)

    const pid1 = getDaemonPid()
    console.log('[Test] Daemon PID after launch:', pid1)

    await closeApp(app)

    // Wait a bit
    await new Promise(resolve => setTimeout(resolve, 2000))

    // Daemon should still be running
    expect(isDaemonRunning()).toBe(true)

    const pid2 = getDaemonPid()
    expect(pid2).toBe(pid1)
    console.log('[Test] Daemon PID after close (same):', pid2)
  })

  test('existing daemon is reused on subsequent launches', async () => {
    // First launch
    const { app: app1, window: window1 } = await launchApp()
    await waitForTerminalReady(window1)
    const daemonPid1 = getDaemonPid()
    console.log('[Test] First launch daemon PID:', daemonPid1)
    await closeApp(app1)

    // Wait a bit
    await new Promise(resolve => setTimeout(resolve, 1000))

    // Second launch
    const { app: app2, window: window2 } = await launchApp()
    await waitForTerminalReady(window2)
    const daemonPid2 = getDaemonPid()
    console.log('[Test] Second launch daemon PID:', daemonPid2)
    await closeApp(app2)

    // Should be same daemon process
    expect(daemonPid1).toBe(daemonPid2)
  })

  test('terminal works after daemon restart', async () => {
    // Launch app
    const { app, window } = await launchApp()
    await waitForTerminalReady(window)

    // Type command
    await window.keyboard.type('echo "BEFORE_DAEMON_RESTART"')
    await window.keyboard.press('Enter')
    await window.waitForTimeout(500)

    // Manually kill daemon
    console.log('[Test] Killing daemon...')
    killDaemon()
    await new Promise(resolve => setTimeout(resolve, 1000))

    expect(isDaemonRunning()).toBe(false)

    // Close app
    await closeApp(app)
    await new Promise(resolve => setTimeout(resolve, 500))

    // Relaunch app (should start new daemon)
    const { app: app2, window: window2 } = await launchApp()
    await waitForTerminalReady(window2)

    // Wait for daemon to start
    await waitForDaemon(10000)
    expect(isDaemonRunning()).toBe(true)

    await window2.waitForTimeout(1000)

    // New terminal should work (old session is lost, that's expected)
    await window2.keyboard.type('echo "AFTER_DAEMON_RESTART"')
    await window2.keyboard.press('Enter')
    await window2.waitForTimeout(1000)

    const content = await window2.evaluate(() => {
      const rows = document.querySelectorAll('.xterm-rows > div')
      return Array.from(rows).map(row => row.textContent || '').join('\n')
    })
    console.log('[Test] Content after daemon restart:', content.substring(0, 200))

    // Should have new command
    expect(content).toContain('AFTER_DAEMON_RESTART')

    await closeApp(app2)
  })

  test('daemon creates socket file', async () => {
    const { app, window } = await launchApp()
    await waitForDaemon()

    // Check socket file exists
    expect(isDaemonRunning()).toBe(true)

    await closeApp(app)
  })

  test('daemon creates PID file', async () => {
    const { app, window } = await launchApp()
    await waitForDaemon()

    const pid = getDaemonPid()
    expect(pid).not.toBeNull()
    expect(pid).toBeGreaterThan(0)

    // Verify process is actually running
    let processExists = false
    try {
      process.kill(pid!, 0) // Signal 0 just checks if process exists
      processExists = true
    } catch {
      processExists = false
    }
    expect(processExists).toBe(true)

    await closeApp(app)
  })
})
