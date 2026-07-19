/**
 * How many bytes the renderer is willing to pull down for a single file read.
 *
 * The daemon's ReadFile streams without a size ceiling — it is the caller that
 * decides what it can hold. Main enforces this budget against the stream header,
 * so an oversize file is rejected before any content crosses the wire.
 *
 * Note this is a *transfer* budget, unrelated to the daemon's poll-backed watch
 * cap: a file can be too large to poll-watch yet still be readable, and vice versa.
 */
export const MAX_READ_FILE_BYTES = 1024 * 1024 // 1MB
