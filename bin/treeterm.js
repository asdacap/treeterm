#!/usr/bin/env node

const { spawn } = require('child_process')
const path = require('path')
const fs = require('fs')

// Get the path to the electron executable
const electronPath = require('electron')

// Get the path to the app's main entry point
const appPath = path.join(__dirname, '..')

// Parse arguments - find first non-flag argument
const args = process.argv.slice(2)
let workspacePath = null
const electronArgs = [appPath]

for (const arg of args) {
  if (arg === '--help' || arg === '-h') {
    console.log('Usage: treeterm [directory]')
    console.log('')
    console.log('Open TreeTerm with an optional workspace directory.')
    process.exit(0)
  }
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
