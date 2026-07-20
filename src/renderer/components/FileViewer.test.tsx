// @vitest-environment jsdom
import React, { useEffect } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { WorkspaceFilesystemApi, WorkspaceStore } from '../types'
import { FileViewer } from './FileViewer'

const filesystem = vi.hoisted(() => ({ readFile: vi.fn() }))

// A stand-in for the Monaco editor instance, recording the scroll calls the component makes.
const fakeEditor = vi.hoisted(() => ({
  setScrollTop: vi.fn(),
  revealLineInCenter: vi.fn(),
  onDidScrollChange: vi.fn(),
  onMouseDown: vi.fn(),
  createDecorationsCollection: vi.fn(() => ({ clear: vi.fn() })),
  changeViewZones: vi.fn((cb: (accessor: unknown) => void) => {
    cb({ addZone: () => 'zone-1', removeZone: () => undefined })
  }),
}))

const fakeMonaco = vi.hoisted(() => ({
  editor: { MouseTargetType: { GUTTER_LINE_NUMBERS: 3 } },
}))

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

function MockEditor({
  language,
  onMount,
}: {
  language: string
  onMount?: (e: unknown, m: unknown) => void
}): React.JSX.Element {
  useEffect(() => {
    onMount?.(fakeEditor, fakeMonaco)
    // Mount is a one-shot handshake with Monaco; re-running it would double-register listeners.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return <div data-testid="monaco-editor" data-language={language} />
}

vi.mock('@monaco-editor/react', () => ({ default: MockEditor }))

function makeWorkspace(): WorkspaceStore {
  return createStore(() => ({
    workspace: { id: 'workspace-1', path: '/repo' },
    addTab: vi.fn(),
    connectionId: 'connection-1',
  })) as unknown as WorkspaceStore
}

describe('FileViewer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

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

  describe('scroll restoration', () => {
    function mockFile(): void {
      filesystem.readFile.mockResolvedValue({
        success: true,
        file: { path: '/repo/src/app.ts', content: 'const a = 1', size: 11, language: 'plaintext' },
      })
    }

    it('restores the persisted scroll offset once the file has loaded', async () => {
      mockFile()

      render(
        <FileViewer
          workspace={makeWorkspace()}
          filePath="src/app.ts"
          initialScrollTop={4200}
          onLineClick={vi.fn()}
        />
      )

      await waitFor(() => { expect(fakeEditor.setScrollTop).toHaveBeenCalledWith(4200) })
    })

    it('does not re-apply the offset when the inline comment input opens', async () => {
      mockFile()
      const workspace = makeWorkspace()

      const { rerender } = render(
        <FileViewer
          workspace={workspace}
          filePath="src/app.ts"
          initialScrollTop={4200}
          onLineClick={vi.fn()}
          onScrollToLineUsed={() => undefined}
          inlineCommentInput={null}
        />
      )

      await waitFor(() => { expect(fakeEditor.setScrollTop).toHaveBeenCalledTimes(1) })

      // Clicking a line number opens the comment input, which re-renders the viewer. Callers pass
      // fresh callback identities on each render, so the restore effect must not key off them.
      rerender(
        <FileViewer
          workspace={workspace}
          filePath="src/app.ts"
          initialScrollTop={4200}
          onLineClick={vi.fn()}
          onScrollToLineUsed={() => undefined}
          inlineCommentInput={{ lineNumber: 7 }}
          onCommentSubmit={vi.fn()}
          onCommentCancel={vi.fn()}
        />
      )

      // The stale offset must not be re-asserted — that is what scrolled the file to the end.
      expect(fakeEditor.setScrollTop).toHaveBeenCalledTimes(1)
    })

    it('reveals a requested line instead of restoring the offset', async () => {
      mockFile()

      render(
        <FileViewer
          workspace={makeWorkspace()}
          filePath="src/app.ts"
          initialScrollTop={4200}
          scrollToLine={12}
          onScrollToLineUsed={vi.fn()}
          onLineClick={vi.fn()}
        />
      )

      await waitFor(() => { expect(fakeEditor.revealLineInCenter).toHaveBeenCalledWith(12) })
      expect(fakeEditor.setScrollTop).not.toHaveBeenCalled()
    })
  })
})
