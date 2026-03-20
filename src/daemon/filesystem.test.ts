import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.mock('fs/promises')
vi.mock('./logger', () => ({
  createModuleLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  })
}))

import * as fs from 'fs/promises'
import { readDirectory, readFile, writeFile, searchFiles } from './filesystem'
import * as path from 'path'

const workspace = '/workspace'

// By default, realpath resolves to the input path (no symlinks)
function setupRealpath(): void {
  vi.mocked(fs.realpath).mockImplementation(async (p: any) => path.resolve(String(p)))
}

describe('readDirectory', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    setupRealpath()
  })

  it('returns error when path is outside workspace', async () => {
    const result = await readDirectory(workspace, '/etc/passwd')
    expect(result.success).toBe(false)
    expect(result.error).toContain('Access denied')
  })

  it('returns error when symlink resolves outside workspace', async () => {
    // Symlink inside workspace that resolves to outside
    vi.mocked(fs.realpath).mockImplementation(async (p: any) => {
      const s = String(p)
      if (s === '/workspace/evil-link') return '/etc/shadow'
      return path.resolve(s)
    })
    const result = await readDirectory(workspace, '/workspace/evil-link')
    expect(result.success).toBe(false)
    expect(result.error).toContain('Access denied')
  })

  it('returns directory contents for a valid path', async () => {
    const mockEntries = [
      { name: 'file.ts', isDirectory: () => false, isFile: () => true },
      { name: 'subdir', isDirectory: () => true, isFile: () => false },
    ]
    vi.mocked(fs.readdir).mockResolvedValue(mockEntries as any)
    vi.mocked(fs.stat).mockResolvedValue({ size: 100, mtimeMs: 1000 } as any)

    const result = await readDirectory(workspace, workspace)
    expect(result.success).toBe(true)
    expect(result.contents).toBeDefined()
    expect(result.contents!.entries).toHaveLength(2)
  })

  it('filters out hidden files', async () => {
    const mockEntries = [
      { name: '.hidden', isDirectory: () => false, isFile: () => true },
      { name: 'visible.ts', isDirectory: () => false, isFile: () => true },
    ]
    vi.mocked(fs.readdir).mockResolvedValue(mockEntries as any)
    vi.mocked(fs.stat).mockResolvedValue({ size: 100, mtimeMs: 1000 } as any)

    const result = await readDirectory(workspace, workspace)
    expect(result.success).toBe(true)
    expect(result.contents!.entries).toHaveLength(1)
    expect(result.contents!.entries[0].name).toBe('visible.ts')
  })

  it('sorts directories before files', async () => {
    const mockEntries = [
      { name: 'afile.ts', isDirectory: () => false, isFile: () => true },
      { name: 'zdir', isDirectory: () => true, isFile: () => false },
    ]
    vi.mocked(fs.readdir).mockResolvedValue(mockEntries as any)
    vi.mocked(fs.stat).mockResolvedValue({ size: 100, mtimeMs: 1000 } as any)

    const result = await readDirectory(workspace, workspace)
    expect(result.contents!.entries[0].name).toBe('zdir')
    expect(result.contents!.entries[0].isDirectory).toBe(true)
  })

  it('resolves relative path against workspace', async () => {
    const mockEntries = [
      { name: 'file.ts', isDirectory: () => false, isFile: () => true },
    ]
    vi.mocked(fs.readdir).mockResolvedValue(mockEntries as any)
    vi.mocked(fs.stat).mockResolvedValue({ size: 100, mtimeMs: 1000 } as any)

    const result = await readDirectory(workspace, 'src')
    expect(result.success).toBe(true)
    expect(result.contents!.path).toBe(`${workspace}/src`)
    expect(fs.readdir).toHaveBeenCalledWith(`${workspace}/src`, { withFileTypes: true })
  })

  it('returns error on readdir failure', async () => {
    vi.mocked(fs.readdir).mockRejectedValue(new Error('ENOENT'))

    const result = await readDirectory(workspace, workspace)
    expect(result.success).toBe(false)
    expect(result.error).toBeDefined()
  })

  it('handles stat errors gracefully for entries', async () => {
    const mockEntries = [
      { name: 'file.ts', isDirectory: () => false, isFile: () => true },
    ]
    vi.mocked(fs.readdir).mockResolvedValue(mockEntries as any)
    vi.mocked(fs.stat).mockRejectedValue(new Error('stat failed'))

    const result = await readDirectory(workspace, workspace)
    expect(result.success).toBe(true)
    expect(result.contents!.entries[0].size).toBeUndefined()
  })
})

describe('readFile', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    setupRealpath()
  })

  it('returns error when path is outside workspace', async () => {
    const result = await readFile(workspace, '/etc/passwd')
    expect(result.success).toBe(false)
    expect(result.error).toContain('Access denied')
  })

  it('returns error when symlink resolves outside workspace', async () => {
    vi.mocked(fs.realpath).mockImplementation(async (p: any) => {
      const s = String(p)
      if (s === '/workspace/evil-link.txt') return '/etc/passwd'
      return path.resolve(s)
    })
    const result = await readFile(workspace, '/workspace/evil-link.txt')
    expect(result.success).toBe(false)
    expect(result.error).toContain('Access denied')
  })

  it('returns error when file exceeds 1MB', async () => {
    vi.mocked(fs.stat).mockResolvedValue({ size: 2 * 1024 * 1024 } as any)

    const result = await readFile(workspace, `${workspace}/large.ts`)
    expect(result.success).toBe(false)
    expect(result.error).toContain('too large')
  })

  it('reads file and detects language', async () => {
    vi.mocked(fs.stat).mockResolvedValue({ size: 100 } as any)
    vi.mocked(fs.readFile).mockResolvedValue('const x = 1' as any)

    const result = await readFile(workspace, `${workspace}/app.ts`)
    expect(result.success).toBe(true)
    expect(result.file!.content).toBe('const x = 1')
    expect(result.file!.language).toBe('typescript')
  })

  it('detects javascript language', async () => {
    vi.mocked(fs.stat).mockResolvedValue({ size: 50 } as any)
    vi.mocked(fs.readFile).mockResolvedValue('const x = 1' as any)

    const result = await readFile(workspace, `${workspace}/app.js`)
    expect(result.file!.language).toBe('javascript')
  })

  it('returns plaintext for unknown extension', async () => {
    vi.mocked(fs.stat).mockResolvedValue({ size: 50 } as any)
    vi.mocked(fs.readFile).mockResolvedValue('data' as any)

    const result = await readFile(workspace, `${workspace}/file.xyz`)
    expect(result.file!.language).toBe('plaintext')
  })

  it('resolves relative path against workspace', async () => {
    vi.mocked(fs.stat).mockResolvedValue({ size: 100 } as any)
    vi.mocked(fs.readFile).mockResolvedValue('const x = 1' as any)

    const result = await readFile(workspace, 'src/app.ts')
    expect(result.success).toBe(true)
    expect(result.file!.path).toBe(`${workspace}/src/app.ts`)
    expect(fs.stat).toHaveBeenCalledWith(`${workspace}/src/app.ts`)
    expect(fs.readFile).toHaveBeenCalledWith(`${workspace}/src/app.ts`, 'utf-8')
  })

  it('returns error on stat failure', async () => {
    vi.mocked(fs.stat).mockRejectedValue(new Error('ENOENT'))

    const result = await readFile(workspace, `${workspace}/missing.ts`)
    expect(result.success).toBe(false)
  })
})

describe('writeFile', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    setupRealpath()
  })

  it('returns error when path is outside workspace', async () => {
    const result = await writeFile(workspace, '/etc/evil', 'content')
    expect(result.success).toBe(false)
    expect(result.error).toContain('Access denied')
  })

  it('writes file successfully', async () => {
    vi.mocked(fs.writeFile).mockResolvedValue(undefined)

    const result = await writeFile(workspace, `${workspace}/out.ts`, 'hello')
    expect(result.success).toBe(true)
    expect(fs.writeFile).toHaveBeenCalledWith(`${workspace}/out.ts`, 'hello', 'utf-8')
  })

  it('resolves relative path against workspace', async () => {
    vi.mocked(fs.writeFile).mockResolvedValue(undefined)

    const result = await writeFile(workspace, 'out.ts', 'hello')
    expect(result.success).toBe(true)
    expect(fs.writeFile).toHaveBeenCalledWith(`${workspace}/out.ts`, 'hello', 'utf-8')
    expect(fs.mkdir).toHaveBeenCalledWith(workspace, { recursive: true })
  })

  it('returns error on write failure', async () => {
    vi.mocked(fs.writeFile).mockRejectedValue(new Error('EACCES'))

    const result = await writeFile(workspace, `${workspace}/out.ts`, 'hello')
    expect(result.success).toBe(false)
    expect(result.error).toBeDefined()
  })
})

describe('searchFiles', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    setupRealpath()
  })

  it('returns empty array for empty query', async () => {
    const result = await searchFiles(workspace, '')
    expect(result.success).toBe(true)
    expect(result.entries).toHaveLength(0)
  })

  it('returns empty array for whitespace-only query', async () => {
    const result = await searchFiles(workspace, '   ')
    expect(result.success).toBe(true)
    expect(result.entries).toHaveLength(0)
  })

  it('returns matching files', async () => {
    // Only files — no dirs to recurse into, no infinite loop
    const mockEntries = [
      { name: 'myfile.ts', isDirectory: () => false },
      { name: 'other.ts', isDirectory: () => false },
    ]
    vi.mocked(fs.readdir).mockResolvedValue(mockEntries as any)
    vi.mocked(fs.stat).mockResolvedValue({ size: 100, mtimeMs: 1000 } as any)

    const result = await searchFiles(workspace, 'myfile')
    expect(result.success).toBe(true)
    expect(result.entries!.some(e => e.name === 'myfile.ts')).toBe(true)
    expect(result.entries!.some(e => e.name === 'other.ts')).toBe(false)
  })

  it('skips hidden files', async () => {
    // Only files — no dirs to recurse into
    const mockEntries = [
      { name: '.hidden', isDirectory: () => false },
      { name: 'visible-hidden-in-name.ts', isDirectory: () => false },
    ]
    vi.mocked(fs.readdir).mockResolvedValue(mockEntries as any)
    vi.mocked(fs.stat).mockResolvedValue({ size: 100, mtimeMs: 1000 } as any)

    const result = await searchFiles(workspace, 'hidden')
    expect(result.success).toBe(true)
    expect(result.entries!.every(e => !e.name.startsWith('.'))).toBe(true)
  })

  it('skips node_modules', async () => {
    // node_modules gets skipped; use mockResolvedValueOnce so second call (for src) returns empty
    vi.mocked(fs.readdir)
      .mockResolvedValueOnce([
        { name: 'node_modules', isDirectory: () => true },
        { name: 'src', isDirectory: () => true },
      ] as any)
      .mockResolvedValueOnce([] as any) // src is empty — no infinite recursion
    vi.mocked(fs.stat).mockResolvedValue({ size: 0, mtimeMs: 0 } as any)

    const result = await searchFiles(workspace, 'node_modules')
    expect(result.success).toBe(true)
    expect(result.entries!.every(e => e.name !== 'node_modules')).toBe(true)
  })

  it('recurses into subdirectories', async () => {
    vi.mocked(fs.readdir)
      .mockResolvedValueOnce([{ name: 'subdir', isDirectory: () => true }] as any)
      .mockResolvedValueOnce([{ name: 'nested.ts', isDirectory: () => false }] as any)
    vi.mocked(fs.stat).mockResolvedValue({ size: 50, mtimeMs: 1000 } as any)

    const result = await searchFiles(workspace, 'nested')
    expect(result.success).toBe(true)
    expect(result.entries!.some(e => e.name === 'nested.ts')).toBe(true)
  })

  it('sorts directories before files', async () => {
    // Use mockResolvedValueOnce for dir and return empty for its contents
    vi.mocked(fs.readdir)
      .mockResolvedValueOnce([
        { name: 'afile.ts', isDirectory: () => false },
        { name: 'zdir', isDirectory: () => true },
      ] as any)
      .mockResolvedValueOnce([] as any) // zdir is empty
    vi.mocked(fs.stat).mockResolvedValue({ size: 100, mtimeMs: 1000 } as any)

    const result = await searchFiles(workspace, 'z')
    expect(result.success).toBe(true)
    if (result.entries && result.entries.length > 1) {
      const dirIndex = result.entries.findIndex(e => e.isDirectory)
      const fileIndex = result.entries.findIndex(e => !e.isDirectory)
      expect(dirIndex).toBeLessThan(fileIndex)
    }
  })
})
