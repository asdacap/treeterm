import { describe, it, expect } from 'vitest'
import {
  IMAGE_LANGUAGE,
  MODEL_LANGUAGE,
  ModelFormat,
  imageMimeType,
  isBinaryContent,
  isImagePath,
  isModelPath,
  isRawBytesPath,
  modelFormat,
  readLanguage,
} from './fileContentKind'

describe('fileContentKind', () => {
  it('recognises image extensions case-insensitively', () => {
    expect(isImagePath('a/logo.PNG')).toBe(true)
    expect(isImagePath('a/icon.svg')).toBe(true)
    expect(isImagePath('a/app.ts')).toBe(false)
    expect(isImagePath('Makefile')).toBe(false)
  })

  it('maps image extensions to mime types', () => {
    expect(imageMimeType('photo.jpg')).toBe('image/jpeg')
    expect(imageMimeType('icon.svg')).toBe('image/svg+xml')
    expect(imageMimeType('data.bin')).toBe('application/octet-stream')
  })

  it('recognises 3D model extensions case-insensitively', () => {
    expect(isModelPath('parts/bracket.STL')).toBe(true)
    expect(isModelPath('scene.glb')).toBe(true)
    expect(isModelPath('print.3mf')).toBe(true)
    expect(isModelPath('logo.png')).toBe(false)
    expect(isModelPath('README')).toBe(false)
  })

  it('maps a model path to its format, and refuses any other path', () => {
    expect(modelFormat('a/b/Bracket.Stl')).toBe(ModelFormat.Stl)
    expect(modelFormat('mesh.obj')).toBe(ModelFormat.Obj)
    expect(modelFormat('scan.ply')).toBe(ModelFormat.Ply)
    expect(modelFormat('print.3mf')).toBe(ModelFormat.ThreeMf)
    expect(modelFormat('scene.gltf')).toBe(ModelFormat.Gltf)
    expect(() => modelFormat('notes.txt')).toThrow('Not a 3D model file: notes.txt')
  })

  it('reads images and models as raw bytes, everything else as text', () => {
    expect(isRawBytesPath('logo.png')).toBe(true)
    expect(isRawBytesPath('part.stl')).toBe(true)
    expect(isRawBytesPath('main.ts')).toBe(false)
  })

  it('tags raw-bytes reads by kind and keeps the daemon language otherwise', () => {
    expect(readLanguage('logo.png', 'plaintext')).toBe(IMAGE_LANGUAGE)
    expect(readLanguage('part.stl', 'plaintext')).toBe(MODEL_LANGUAGE)
    expect(readLanguage('main.ts', 'typescript')).toBe('typescript')
  })

  it('detects binary content by a leading NUL byte', () => {
    expect(isBinaryContent('hello\nworld')).toBe(false)
    expect(isBinaryContent('')).toBe(false)
    expect(isBinaryContent('\x89PNG\r\n\x1a\n\0\0')).toBe(true)
    expect(isBinaryContent(`${'x'.repeat(8000)}\0`)).toBe(false)
  })
})
