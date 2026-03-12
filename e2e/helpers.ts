/**
 * E2E Test Helpers
 */

import { _electron as electron, type ElectronApplication, type Page } from 'playwright'
import { execSync, spawn } from 'child_process'
import * as path from 'path'
import * as fs from 'fs'
import * as os from 'os'

export async function launchApp(workspacePath?: string): Promise<{ app: ElectronApplication; window: Page }> {
  // Use current directory as test workspace if not specified
  const workspace = workspacePath || process.cwd()

  const app = await electron.launch({
    args: [
      path.join(__dirname, '../out/main/index.js'),
      `--workspace=${workspace}`
    ],
    env: {
      ...process.env,
      NODE_ENV: 'test'
    }
  })

  const window = await app.firstWindow()
  await window.waitForLoadState('domcontentloaded')

  // Wait a bit for app to initialize
  await window.waitForTimeout(1000)

  return { app, window }
}

export async function closeApp(app: ElectronApplication): Promise<void> {
  await app.close()
}

export function getDaemonSocketPath(): string {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  return path.join(os.tmpdir(), `treeterm-${uid}`, 'daemon.sock')
}

export function isDaemonRunning(): boolean {
  const socketPath = getDaemonSocketPath()
  return fs.existsSync(socketPath)
}

export function getDaemonPid(): number | null {
  const pidFile = path.join(os.homedir(), '.treeterm', 'daemon.pid')
  if (!fs.existsSync(pidFile)) {
    return null
  }
  const pid = parseInt(fs.readFileSync(pidFile, 'utf-8'), 10)
  return pid
}

export function killDaemon(): void {
  const pid = getDaemonPid()
  if (pid) {
    try {
      process.kill(pid, 'SIGTERM')
      // Wait a bit for daemon to shutdown
      let attempts = 0
      while (isDaemonRunning() && attempts < 10) {
        execSync('sleep 0.5')
        attempts++
      }
    } catch (error) {
      // Expected: ESRCH if process already dead - this is the desired state for cleanup
      // Intentionally not failing loudly here as "already dead" is success for cleanup
    }
  }

  // Clean up socket file if it still exists
  const socketPath = getDaemonSocketPath()
  if (fs.existsSync(socketPath)) {
    fs.unlinkSync(socketPath)
  }

  // Clean up pid file
  const pidFile = path.join(os.homedir(), '.treeterm', 'daemon.pid')
  if (fs.existsSync(pidFile)) {
    fs.unlinkSync(pidFile)
  }
}

export async function waitForDaemon(timeoutMs: number = 5000): Promise<void> {
  const startTime = Date.now()
  while (!isDaemonRunning() && Date.now() - startTime < timeoutMs) {
    await new Promise(resolve => setTimeout(resolve, 100))
  }
  if (!isDaemonRunning()) {
    throw new Error('Daemon did not start in time')
  }
}

export async function waitForTerminalReady(window: Page): Promise<void> {
  await window.waitForSelector('.xterm', { timeout: 10000 })
  // Wait a bit for terminal to be fully initialized
  await window.waitForTimeout(500)
}

export async function getTerminalText(window: Page): Promise<string> {
  // Get text by selecting all from terminal and copying
  await window.focus('.xterm textarea')

  // Try to get text content from the visible terminal
  const text = await window.evaluate(() => {
    // Get the terminal viewport element
    const viewport = document.querySelector('.xterm-viewport')
    if (!viewport) return ''

    // Get all row elements
    const rows = document.querySelectorAll('.xterm-rows > div')
    if (rows.length === 0) return ''

    const lines: string[] = []
    rows.forEach(row => {
      const text = row.textContent || ''
      lines.push(text)
    })

    return lines.join('\n')
  })

  return text
}

export function cleanupTestData(): void {
  const treeTermDir = path.join(os.homedir(), '.treeterm')
  if (fs.existsSync(treeTermDir)) {
    // Don't delete everything, just test-specific files
    const sessionsFile = path.join(treeTermDir, 'sessions.json')
    if (fs.existsSync(sessionsFile)) {
      fs.unlinkSync(sessionsFile)
    }
    const scrollbackDir = path.join(treeTermDir, 'scrollback')
    if (fs.existsSync(scrollbackDir)) {
      const files = fs.readdirSync(scrollbackDir)
      for (const file of files) {
        fs.unlinkSync(path.join(scrollbackDir, file))
      }
    }
  }
}
