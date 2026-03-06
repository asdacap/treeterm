#!/usr/bin/env node

const { spawn } = require('child_process')
const path = require('path')

// Get the path to the electron executable
const electronPath = require('electron')

// Get the path to the app's main entry point
const appPath = path.join(__dirname, '..')

// Spawn electron with the app
const child = spawn(electronPath, [appPath], {
  stdio: 'inherit',
  windowsHide: false
})

child.on('close', (code) => {
  process.exit(code)
})
