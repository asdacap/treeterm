import { isModelPath, isPdfPath } from '../../shared/fileContentKind'

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

/** 3D models (meshes for printing, scanned point clouds) routinely run to tens of megabytes. */
export const MAX_READ_MODEL_BYTES = 64 * 1024 * 1024 // 64MB

/** Scanned documents and exported slide decks are routinely several megabytes, often tens. */
export const MAX_READ_PDF_BYTES = 64 * 1024 * 1024 // 64MB

/** The transfer budget for opening `filePath` in a viewer. */
export function maxReadBytesFor(filePath: string): number {
  if (isModelPath(filePath)) return MAX_READ_MODEL_BYTES
  if (isPdfPath(filePath)) return MAX_READ_PDF_BYTES
  return MAX_READ_FILE_BYTES
}
