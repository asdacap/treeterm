#!/usr/bin/env node
import { spawnSync } from 'node:child_process'

const chunkSize = Number(process.argv[2] ?? 1024)
if (!Number.isFinite(chunkSize) || chunkSize < 1) {
  console.error(`Invalid chunk size: ${process.argv[2]}`)
  process.exit(1)
}

const body = 'a'.repeat(chunkSize)
const trailer = `\ncopied ${chunkSize} bytes`
const text = body + trailer

const result = spawnSync('pbcopy', [], { input: text })
if (result.status !== 0) {
  console.error('pbcopy failed:', result.stderr?.toString())
  process.exit(1)
}

console.log(`Copied ${text.length} bytes total (${chunkSize} 'a' chars + trailer) to clipboard.`)
