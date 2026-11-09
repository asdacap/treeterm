const IMAGE_MIME_TYPES: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  avif: 'image/avif',
  svg: 'image/svg+xml',
}

export enum ModelFormat {
  Stl = 'stl',
  Obj = 'obj',
  Ply = 'ply',
  ThreeMf = '3mf',
  Glb = 'glb',
  Gltf = 'gltf',
}

const MODEL_FORMATS: Record<string, ModelFormat> = {
  stl: ModelFormat.Stl,
  obj: ModelFormat.Obj,
  ply: ModelFormat.Ply,
  '3mf': ModelFormat.ThreeMf,
  glb: ModelFormat.Glb,
  gltf: ModelFormat.Gltf,
}

/** `FileContents.language` of a file read as an image (its content is base64). */
export const IMAGE_LANGUAGE = 'image'
/** `FileContents.language` of a file read as a 3D model (its content is base64). */
export const MODEL_LANGUAGE = 'model'

function extensionOf(filePath: string): string {
  return filePath.split('.').pop()?.toLowerCase() ?? ''
}

/** Images are recognised by extension; their bytes travel base64-encoded. */
export function isImagePath(filePath: string): boolean {
  return extensionOf(filePath) in IMAGE_MIME_TYPES
}

/** Only meaningful for a path that passed `isImagePath`. */
export function imageMimeType(filePath: string): string {
  return IMAGE_MIME_TYPES[extensionOf(filePath)] ?? 'application/octet-stream'
}

/** 3D models are recognised by extension; their bytes travel base64-encoded. */
export function isModelPath(filePath: string): boolean {
  return extensionOf(filePath) in MODEL_FORMATS
}

/** Throws for a path that did not pass `isModelPath`. */
export function modelFormat(filePath: string): ModelFormat {
  const format = MODEL_FORMATS[extensionOf(filePath)]
  if (format === undefined) throw new Error(`Not a 3D model file: ${filePath}`)
  return format
}

/**
 * Files the renderer draws from their raw bytes. A read sends these base64-encoded,
 * since decoding them as UTF-8 text would mangle the bytes.
 */
export function isRawBytesPath(filePath: string): boolean {
  return isImagePath(filePath) || isModelPath(filePath)
}

/** The `FileContents.language` a read reports: raw-bytes files are tagged by kind. */
export function readLanguage(filePath: string, daemonLanguage: string): string {
  if (isImagePath(filePath)) return IMAGE_LANGUAGE
  if (isModelPath(filePath)) return MODEL_LANGUAGE
  return daemonLanguage
}

// Git's own heuristic: a NUL within the first 8000 bytes marks the content binary.
const BINARY_SNIFF_LENGTH = 8000

export function isBinaryContent(content: string): boolean {
  return content.slice(0, BINARY_SNIFF_LENGTH).includes('\0')
}
