// @vitest-environment jsdom
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { FileEntry } from '../types'
import RemoteDirectoryBrowser from './RemoteDirectoryBrowser'

function entry(name: string, isDirectory: boolean): FileEntry {
  return { name, path: `/home/${name}`, relativePath: `home/${name}`, isDirectory }
}

describe('RemoteDirectoryBrowser', () => {
  it('lists only the subdirectories that are not hidden', async () => {
    const readDirectory = vi.fn((path: string) => Promise.resolve({
      success: true as const,
      contents: { path, entries: [entry('.config', true), entry('projects', true), entry('notes.txt', false)] },
    }))

    render(<RemoteDirectoryBrowser readDirectory={readDirectory} initialPath="/home" onSelect={vi.fn()} onCancel={vi.fn()} />)

    expect(await screen.findByText('projects')).toBeDefined()
    expect(screen.queryByText('.config')).toBeNull()
    expect(screen.queryByText('notes.txt')).toBeNull()
    expect(readDirectory).toHaveBeenCalledWith('/home')
  })
})
