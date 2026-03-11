#!/usr/bin/env node

const { spawn } = require('child_process')
const path = require('path')
const fs = require('fs')
const os = require('os')
const grpc = require('@grpc/grpc-js')

// Get the path to the electron executable
const electronPath = require('electron')

// Get the path to the app's main entry point
const appPath = path.join(__dirname, '..')

// Daemon paths
const DAEMON_PID_FILE = path.join(os.homedir(), '.treeterm', 'daemon.pid')

function getDefaultSocketPath() {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  return path.join(os.tmpdir(), `treeterm-${uid}`, 'daemon.sock')
}

// Lightweight gRPC daemon client for CLI
class CliDaemonClient {
  constructor(socketPath) {
    this.socketPath = socketPath
    this.client = null

    // Load generated proto client
    const { TreeTermDaemonClient } = require('../out/generated/treeterm')
    this.TreeTermDaemonClient = TreeTermDaemonClient
  }

  async connect(timeout = 5000) {
    return new Promise((resolve, reject) => {
      const socketUri = `unix://${this.socketPath}`
      const credentials = grpc.credentials.createInsecure()

      this.client = new this.TreeTermDaemonClient(socketUri, credentials)

      this.client.waitForReady(Date.now() + timeout, (error) => {
        if (error) {
          reject(error)
        } else {
          resolve()
        }
      })
    })
  }

  disconnect() {
    if (this.client) {
      this.client.close()
      this.client = null
    }
  }

  async listSessions() {
    return new Promise((resolve, reject) => {
      this.client.listSessions({}, (error, response) => {
        if (error) {
          reject(error)
        } else {
          resolve(response.sessions || [])
        }
      })
    })
  }

  async shutdownDaemon() {
    return new Promise((resolve, reject) => {
      this.client.shutdown({}, (error) => {
        if (error) {
          reject(error)
        } else {
          resolve()
        }
      })
    })
  }
}

// Parse arguments - find first non-flag argument
const args = process.argv.slice(2)
const command = args[0]

if (command === '--help' || command === '-h') {
  console.log('Usage: treeterm [command] [options]')
  console.log('')
  console.log('Commands:')
  console.log('  (no command)          Open TreeTerm GUI')
  console.log('  [directory]           Open TreeTerm GUI with workspace directory')
  console.log('  list-sessions         List all active daemon sessions')
  console.log('  shutdown-daemon       Shutdown the daemon process')
  console.log('  status                Show daemon status')
  console.log('')
  console.log('Options:')
  console.log('  -h, --help            Show this help message')
  process.exit(0)
}

// Handle daemon management commands
if (command === 'list-sessions') {
  ;(async () => {
    const client = new CliDaemonClient(getDefaultSocketPath())
    try {
      await client.connect()
      const sessions = await client.listSessions()

      if (sessions.length === 0) {
        console.log('No active sessions')
      } else {
        console.log(`Active Sessions (${sessions.length}):`)
        console.log('')
        for (const session of sessions) {
          console.log(`Session ID: ${session.id}`)
          console.log(`  Workspaces: ${session.workspaces.length}`)
          for (const workspace of session.workspaces) {
            console.log(`    - ${workspace.path}`)
            if (workspace.gitBranch) {
              console.log(`      Branch: ${workspace.gitBranch}`)
            }
            console.log(`      Tabs: ${workspace.tabs.length}`)
          }
          console.log(`  Attached Clients: ${session.attachedClients}`)
          console.log(`  Last Activity: ${new Date(session.lastActivity).toLocaleString()}`)
          console.log('')
        }
      }

      client.disconnect()
      process.exit(0)
    } catch (error) {
      console.error('Error: Daemon is not running')
      client.disconnect()
      process.exit(1)
    }
  })()
  return
}

if (command === 'shutdown-daemon') {
  ;(async () => {
    const client = new CliDaemonClient(getDefaultSocketPath())
    try {
      await client.connect()
      await client.shutdownDaemon()
      console.log('Daemon shutdown successfully')
      client.disconnect()
      process.exit(0)
    } catch (error) {
      console.error('Error: Daemon is not running')
      client.disconnect()
      process.exit(1)
    }
  })()
  return
}

if (command === 'status') {
  ;(async () => {
    const socketPath = getDefaultSocketPath()

    // Check PID file
    let pid = null
    let processRunning = false

    if (fs.existsSync(DAEMON_PID_FILE)) {
      try {
        pid = parseInt(fs.readFileSync(DAEMON_PID_FILE, 'utf-8'), 10)
        // Check if process is still alive
        process.kill(pid, 0)
        processRunning = true
      } catch {
        // Process not running
        processRunning = false
      }
    }

    // Try connecting to daemon
    const client = new CliDaemonClient(socketPath)
    try {
      await client.connect(2000)
      const sessions = await client.listSessions()

      console.log('Daemon Status: running')
      if (pid) {
        console.log(`PID: ${pid}`)
      }
      console.log(`Socket: ${socketPath}`)
      console.log(`Sessions: ${sessions.length}`)

      client.disconnect()
      process.exit(0)
    } catch (error) {
      console.log('Daemon Status: not running')
      if (pid && processRunning) {
        console.log(`Note: PID file exists (${pid}) but daemon is not responding`)
      }
      client.disconnect()
      process.exit(0)
    }
  })()
  return
}

// Handle workspace argument for GUI mode
let workspacePath = null
const electronArgs = [appPath]

for (const arg of args) {
  if (!arg.startsWith('-') && !workspacePath) {
    workspacePath = arg
  }
}

// Validate and resolve workspace path
if (workspacePath) {
  const resolvedPath = path.resolve(process.cwd(), workspacePath)

  if (!fs.existsSync(resolvedPath)) {
    console.error(`Error: Directory does not exist: ${resolvedPath}`)
    process.exit(1)
  }

  const stat = fs.statSync(resolvedPath)
  if (!stat.isDirectory()) {
    console.error(`Error: Path is not a directory: ${resolvedPath}`)
    process.exit(1)
  }

  electronArgs.push(`--workspace=${resolvedPath}`)
}

// Spawn electron with the app
const child = spawn(electronPath, electronArgs, {
  stdio: 'inherit',
  windowsHide: false
})

child.on('close', (code) => {
  process.exit(code)
})
