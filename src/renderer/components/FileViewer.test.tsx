// @vitest-environment jsdom
import { render, screen } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { describe, expect, it, vi } from 'vitest'
import type { WorkspaceFilesystemApi, WorkspaceStore } from '../types'
import { FileViewer } from './FileViewer'

const filesystem = vi.hoisted(() => ({ readFile: vi.fn() }))

vi.mock('../hooks/useWorkspaceApis', () => ({
  useFilesystemApi: () => filesystem as unknown as WorkspaceFilesystemApi,
  useExecApi: () => ({}),
}))

vi.mock('../monaco-config', () => ({
  monacoNavigationBridge: {
    searchDefinition: null,
    openFileAtLine: null,
    getWorkspacePath: null,
  },
}))

vi.mock('@monaco-editor/react', () => ({
  default: ({ language }: { language: string }) => (
    <div data-testid="monaco-editor" data-language={language} />
  ),
}))

function makeWorkspace(): WorkspaceStore {
  return createStore(() => ({
    workspace: { id: 'workspace-1', path: '/repo' },
    addTab: vi.fn(),
    connectionId: 'connection-1',
  })) as unknown as WorkspaceStore
}

describe('FileViewer', () => {
  it('detects C# from the file path instead of daemon language metadata', async () => {
    filesystem.readFile.mockResolvedValue({
      success: true,
      file: {
        path: '/repo/src/Example.cs',
        content: 'public class Example {}',
        size: 23,
        language: 'plaintext',
      },
    })

    render(<FileViewer workspace={makeWorkspace()} filePath="src/Example.cs" />)

    expect(await screen.findByText('csharp')).toBeTruthy()
    expect(screen.getByTestId('monaco-editor').getAttribute('data-language')).toBe('csharp')
  })
})
