// @vitest-environment jsdom
import React, { useEffect } from 'react'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { WorkspaceFilesystemApi, WorkspaceStore } from '../types'
import { FileViewer } from './FileViewer'
import { REPO_SEARCH_DEBOUNCE_MS } from './SearchPanel'
import { RepoSearchEventType, RepoSearchTool } from '../utils/repoSearch'
import { MAX_READ_FILE_BYTES } from '../lib/fileLimits'

const filesystem = vi.hoisted(() => ({ readFile: vi.fn() }))

// A stand-in for the Monaco editor instance, recording the scroll calls the component makes.
const fakeEditor = vi.hoisted(() => ({
  setScrollTop: vi.fn(),
  revealLineInCenter: vi.fn(),
  onDidScrollChange: vi.fn(),
  onMouseDown: vi.fn(),
  addCommand: vi.fn(),
  addAction: vi.fn(),
  getModel: vi.fn(),
  getSelection: vi.fn(),
  setSelection: vi.fn(),
  createDecorationsCollection: vi.fn(() => ({ clear: vi.fn() })),
  changeViewZones: vi.fn((cb: (accessor: unknown) => void) => {
    cb({ addZone: () => 'zone-1', removeZone: () => undefined })
  }),
}))

const fakeMonaco = vi.hoisted(() => ({
  editor: { MouseTargetType: { GUTTER_LINE_NUMBERS: 3, CONTENT_TEXT: 6 } },
  KeyMod: { CtrlCmd: 2048 },
  KeyCode: { KeyF: 36 },
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

// The real viewer needs WebGL; this stands in for the lazily loaded module.
vi.mock('./ModelViewer', () => ({
  default: ({ base64, format }: { base64: string; format: string }) => (
    <div data-testid="model-viewer" data-base64={base64} data-format={format} />
  ),
}))

const download = vi.hoisted(() => ({ downloadBytes: vi.fn() }))
vi.mock('../utils/downloadFile', () => download)

// The repo search is driven by hand: the test grabs the onEvent callback.
const repoSearch = vi.hoisted(() => ({
  onEvent: null as ((event: unknown) => void) | null,
}))
vi.mock('../utils/repoSearch', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../utils/repoSearch')>()
  return {
    ...actual,
    startRepoSearch: vi.fn((_exec: unknown, _conn: unknown, _cwd: unknown, _query: unknown, onEvent: (event: unknown) => void) => {
      repoSearch.onEvent = onEvent
      return () => { repoSearch.onEvent = null }
    }),
  }
})

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

    render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="src/Example.cs" />)

    expect(await screen.findByText('csharp')).toBeTruthy()
    expect(screen.getByTestId('monaco-editor').getAttribute('data-language')).toBe('csharp')
  })

  it('shows a binary placeholder instead of the editor for binary content', async () => {
    filesystem.readFile.mockResolvedValue({
      success: true,
      file: { path: '/repo/app.bin', content: 'ELF\0\0\x01', size: 6, language: 'plaintext' },
    })

    render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="app.bin" />)

    expect(await screen.findByText('Binary file — not shown as text')).toBeTruthy()
    expect(screen.getByText('binary')).toBeTruthy()
    expect(screen.queryByTestId('monaco-editor')).toBeNull()
    expect(screen.queryByText('⇗ Open in Tab')).toBeNull()
  })

  it('renders an image file as an image with its mime type', async () => {
    filesystem.readFile.mockResolvedValue({
      success: true,
      file: { path: '/repo/logo.jpg', content: 'AAEC', size: 3, language: 'image' },
    })

    render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="logo.jpg" />)

    const img = await screen.findByAltText('logo.jpg')
    expect(img.getAttribute('src')).toBe('data:image/jpeg;base64,AAEC')
  })

  it('downloads an image as its exact bytes', async () => {
    filesystem.readFile.mockResolvedValue({
      success: true,
      file: { path: '/repo/logo.jpg', content: 'AAEC', size: 3, language: 'image' },
    })

    render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="assets/logo.jpg" />)
    fireEvent.click(await screen.findByText('Download'))

    expect(download.downloadBytes).toHaveBeenCalledWith('logo.jpg', new Uint8Array([0, 1, 2]), 'image/jpeg')
  })

  it('shows a 3D model in the model viewer, with download instead of open-in-tab', async () => {
    filesystem.readFile.mockResolvedValue({
      success: true,
      file: { path: '/repo/parts/bracket.stl', content: 'AP8=', size: 2, language: 'model' },
    })

    render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="parts/bracket.stl" />)

    const viewer = await screen.findByTestId('model-viewer')
    expect(viewer.getAttribute('data-base64')).toBe('AP8=')
    expect(viewer.getAttribute('data-format')).toBe('stl')
    expect(screen.getByText('stl', { selector: '.file-viewer-language' })).toBeTruthy()
    expect(screen.queryByText('⇗ Open in Tab')).toBeNull()
    expect(screen.queryByTestId('monaco-editor')).toBeNull()

    fireEvent.click(screen.getByText('Download'))
    expect(download.downloadBytes).toHaveBeenCalledWith('bracket.stl', new Uint8Array([0, 255]), 'model/stl')
  })

  it('never renders the previous file under a newly selected path', async () => {
    filesystem.readFile.mockResolvedValueOnce({
      success: true,
      file: { path: '/repo/part.stl', content: 'AP8=', size: 2, language: 'model' },
    })
    const workspace = makeWorkspace()
    const { rerender } = render(<FileViewer workspace={workspace} onOpenLocation={vi.fn()} filePath="part.stl" />)
    await screen.findByTestId('model-viewer')
    filesystem.readFile.mockReturnValueOnce(new Promise(() => {}))

    rerender(<FileViewer workspace={workspace} onOpenLocation={vi.fn()} filePath="notes.txt" />)

    // The model's bytes are not handed on as a notes.txt model (modelFormat would throw).
    expect(screen.getByText('Loading...')).toBeTruthy()
    expect(screen.queryByTestId('model-viewer')).toBeNull()
  })

  describe('read failures', () => {
    const OVERSIZE = Math.round(1.4 * 1024 * 1024)

    function tooLarge(): unknown {
      return { success: false, error: 'File too large to open (1.4MB, limit 1.0MB)', tooLarge: true, size: OVERSIZE }
    }

    it('reads with the default budget for the file kind', async () => {
      filesystem.readFile.mockResolvedValue({
        success: true,
        file: { path: '/repo/a.ts', content: 'const a = 1', size: 11, language: 'typescript' },
      })

      render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="a.ts" />)

      await screen.findByTestId('monaco-editor')
      expect(filesystem.readFile).toHaveBeenCalledWith('a.ts', MAX_READ_FILE_BYTES)
    })

    it('offers to open an oversize file anyway, re-reading with a budget that admits it', async () => {
      filesystem.readFile.mockResolvedValueOnce(tooLarge())
      filesystem.readFile.mockResolvedValueOnce({
        success: true,
        file: { path: '/repo/big.log', content: 'lots of log', size: OVERSIZE, language: 'plaintext' },
      })

      render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="big.log" />)

      expect(await screen.findByText('File too large to open (1.4MB, limit 1.0MB)')).toBeTruthy()
      fireEvent.click(screen.getByText('Open anyway (1.4MB)'))

      expect(await screen.findByTestId('monaco-editor')).toBeTruthy()
      expect(filesystem.readFile).toHaveBeenLastCalledWith('big.log', OVERSIZE)
    })

    it('drops the raised budget when another file is selected', async () => {
      filesystem.readFile.mockResolvedValueOnce(tooLarge())
      filesystem.readFile.mockResolvedValueOnce({
        success: true,
        file: { path: '/repo/big.log', content: 'lots of log', size: OVERSIZE, language: 'plaintext' },
      })
      const workspace = makeWorkspace()
      const { rerender } = render(<FileViewer workspace={workspace} onOpenLocation={vi.fn()} filePath="big.log" />)
      fireEvent.click(await screen.findByText('Open anyway (1.4MB)'))
      await screen.findByTestId('monaco-editor')
      filesystem.readFile.mockResolvedValueOnce(tooLarge())

      rerender(<FileViewer workspace={workspace} onOpenLocation={vi.fn()} filePath="other.log" />)

      expect(await screen.findByText('Open anyway (1.4MB)')).toBeTruthy()
      expect(filesystem.readFile).toHaveBeenLastCalledWith('other.log', MAX_READ_FILE_BYTES)
    })

    it('shows other failures without an override', async () => {
      filesystem.readFile.mockResolvedValue({ success: false, error: 'Permission denied' })

      render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="secret.txt" />)

      expect(await screen.findByText('Permission denied')).toBeTruthy()
      expect(screen.queryByText(/Open anyway/)).toBeNull()
    })

    it('falls back to a generic message when a failure carries none', async () => {
      filesystem.readFile.mockResolvedValue({ success: false, error: '' })

      render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="a.txt" />)

      expect(await screen.findByText('Failed to load file')).toBeTruthy()
    })

    it('shows a thrown read error', async () => {
      filesystem.readFile.mockRejectedValue(new Error('Not connected to daemon'))

      render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="a.txt" />)

      expect(await screen.findByText('Error loading file: Not connected to daemon')).toBeTruthy()
    })

    it('discards a superseded read that lands after the next file', async () => {
      let resolveSlow: (value: unknown) => void = () => {}
      let rejectSlow: (err: Error) => void = () => {}
      filesystem.readFile.mockReturnValueOnce(new Promise((resolve) => { resolveSlow = resolve }))
      filesystem.readFile.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectSlow = reject }))
      filesystem.readFile.mockResolvedValueOnce({
        success: true,
        file: { path: '/repo/c.ts', content: 'const c = 3', size: 11, language: 'typescript' },
      })
      const workspace = makeWorkspace()
      const { rerender } = render(<FileViewer workspace={workspace} onOpenLocation={vi.fn()} filePath="a.ts" />)
      rerender(<FileViewer workspace={workspace} onOpenLocation={vi.fn()} filePath="b.ts" />)
      rerender(<FileViewer workspace={workspace} onOpenLocation={vi.fn()} filePath="c.ts" />)
      expect(await screen.findByText('typescript')).toBeTruthy()

      await act(async () => {
        resolveSlow({ success: false, error: 'stale failure' })
        rejectSlow(new Error('stale throw'))
        await Promise.resolve()
      })

      expect(screen.queryByText('stale failure')).toBeNull()
      expect(screen.queryByText(/stale throw/)).toBeNull()
      expect(screen.getByTestId('monaco-editor')).toBeTruthy()
    })
  })

  it('offers no download for a text file, whose bytes were decoded on read', async () => {
    filesystem.readFile.mockResolvedValue({
      success: true,
      file: { path: '/repo/a.ts', content: 'const a = 1', size: 11, language: 'typescript' },
    })

    render(<FileViewer workspace={makeWorkspace()} onOpenLocation={vi.fn()} filePath="a.ts" />)

    expect(await screen.findByText('⇗ Open in Tab')).toBeTruthy()
    expect(screen.queryByText('Download')).toBeNull()
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
          workspace={makeWorkspace()} onOpenLocation={vi.fn()}
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
          onOpenLocation={vi.fn()}
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
          onOpenLocation={vi.fn()}
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
          workspace={makeWorkspace()} onOpenLocation={vi.fn()}
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

  describe('search', () => {
    const content = 'const needle = 1\nother line\nneedle(needle)'

    const cursor = { getPosition: () => ({ lineNumber: 1, column: 8 }) }
    const getValueInRange = vi.fn()
    const getWordAtPosition = vi.fn()

    async function renderLoaded(onOpenLocation = vi.fn()): Promise<void> {
      filesystem.readFile.mockResolvedValue({
        success: true,
        file: { path: '/repo/src/app.ts', content, size: content.length, language: 'plaintext' },
      })
      getValueInRange.mockReturnValue('')
      getWordAtPosition.mockReturnValue({ word: 'needle' })
      fakeEditor.getModel.mockReturnValue({ getWordAtPosition, getValueInRange })
      fakeEditor.getSelection.mockReturnValue(cursor)
      render(<FileViewer workspace={makeWorkspace()} filePath="/repo/src/app.ts" onOpenLocation={onOpenLocation} />)
      await screen.findByTestId('monaco-editor')
    }

    // Right-click > Search
    function searchFromMenu(): void {
      const action = fakeEditor.addAction.mock.calls.at(-1)?.[0] as { label: string; contextMenuGroupId: string; run: () => void }
      expect(action.label).toBe('Search')
      expect(action.contextMenuGroupId).toBe('9_cutcopypaste')
      act(() => { action.run() })
    }

    function pressCmdF(): void {
      const command = fakeEditor.addCommand.mock.calls.at(-1)?.[1] as () => void
      act(() => { command() })
    }

    it('right-click > Search on a word opens the panel with every occurrence in the file', async () => {
      await renderLoaded()
      expect(screen.queryByPlaceholderText('Search')).toBeNull()

      searchFromMenu()

      expect(getWordAtPosition).toHaveBeenCalledWith({ lineNumber: 1, column: 8 })
      expect(screen.getByPlaceholderText<HTMLInputElement>('Search').value).toBe('needle')
      expect(screen.getByText('In this file')).toBeTruthy()
      expect(screen.getByText('3 found')).toBeTruthy()
      expect(screen.getAllByText('needle(needle)')).toHaveLength(2)
    })

    it('right-click > Search uses a single-line selection over the word', async () => {
      await renderLoaded()
      getValueInRange.mockReturnValue('other line')
      searchFromMenu()
      expect(screen.getByPlaceholderText<HTMLInputElement>('Search').value).toBe('other line')
    })

    it('right-click > Search falls back to the word for a multi-line selection', async () => {
      await renderLoaded()
      getValueInRange.mockReturnValue('a\nb')
      searchFromMenu()
      expect(screen.getByPlaceholderText<HTMLInputElement>('Search').value).toBe('needle')
    })

    it('right-click > Search on whitespace does nothing', async () => {
      await renderLoaded()
      getWordAtPosition.mockReturnValue(null)
      searchFromMenu()
      expect(screen.queryByPlaceholderText('Search')).toBeNull()
    })

    it('right-click > Search without a model does nothing', async () => {
      await renderLoaded()
      fakeEditor.getModel.mockReturnValue(null)
      searchFromMenu()
      expect(screen.queryByPlaceholderText('Search')).toBeNull()
    })

    it('Cmd+F opens the panel seeded with the selection', async () => {
      await renderLoaded()
      getValueInRange.mockReturnValue('other')
      pressCmdF()
      expect(screen.getByPlaceholderText<HTMLInputElement>('Search').value).toBe('other')
    })

    it('Cmd+F without a selection opens an empty panel, and keeps the query when already open', async () => {
      await renderLoaded()
      fakeEditor.getSelection.mockReturnValue(null)
      pressCmdF()
      const input = screen.getByPlaceholderText<HTMLInputElement>('Search')
      expect(input.value).toBe('')
      fireEvent.change(input, { target: { value: 'needle' } })
      pressCmdF()
      expect(input.value).toBe('needle')
    })

    it('clicking or stepping to an occurrence reveals and selects it', async () => {
      await renderLoaded()
      searchFromMenu()

      fireEvent.click(screen.getByText('const needle = 1'))
      expect(fakeEditor.revealLineInCenter).toHaveBeenLastCalledWith(1)
      expect(fakeEditor.setSelection).toHaveBeenLastCalledWith({ startLineNumber: 1, startColumn: 7, endLineNumber: 1, endColumn: 13 })
      expect(screen.getByText('1/3')).toBeTruthy()

      const input = screen.getByPlaceholderText<HTMLInputElement>('Search')
      fireEvent.keyDown(input, { key: 'Enter' })
      expect(fakeEditor.setSelection).toHaveBeenLastCalledWith({ startLineNumber: 3, startColumn: 1, endLineNumber: 3, endColumn: 7 })
      fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
      fireEvent.keyDown(input, { key: 'Enter', shiftKey: true })
      expect(screen.getByText('3/3')).toBeTruthy()
    })

    it('stepping backwards with nothing selected goes to the last occurrence', async () => {
      await renderLoaded()
      searchFromMenu()
      fireEvent.keyDown(screen.getByPlaceholderText('Search'), { key: 'Enter', shiftKey: true })
      expect(screen.getByText('3/3')).toBeTruthy()
    })

    it('stepping does nothing when there are no occurrences', async () => {
      await renderLoaded()
      searchFromMenu()
      const input = screen.getByPlaceholderText<HTMLInputElement>('Search')
      fireEvent.change(input, { target: { value: 'absent' } })
      fireEvent.keyDown(input, { key: 'Enter' })
      expect(fakeEditor.setSelection).not.toHaveBeenCalled()
      expect(screen.getByText('No matches')).toBeTruthy()
    })

    it('opens repo results in other files and leaves out the current file', async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true })
      try {
        const onOpenLocation = vi.fn()
        await renderLoaded(onOpenLocation)
        searchFromMenu()
        act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
        act(() => {
          repoSearch.onEvent?.({
            type: RepoSearchEventType.Hits,
            hits: [
              { filePath: 'src/app.ts', lineNumber: 1, column: 7, preview: 'this file again' },
              { filePath: 'src/other.ts', lineNumber: 4, column: 2, preview: 'needle in other' },
            ],
          })
          repoSearch.onEvent?.({ type: RepoSearchEventType.Done, tool: RepoSearchTool.GitGrep, truncated: true })
        })

        expect(screen.queryByText('this file again')).toBeNull()
        expect(screen.getByText('1 found (stopped at the result limit) · git grep')).toBeTruthy()
        fireEvent.click(screen.getByText('needle in other'))
        expect(onOpenLocation).toHaveBeenCalledWith('src/other.ts', 4)
      } finally {
        vi.useRealTimers()
      }
    })

    it('closing the panel cancels the repo search', async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true })
      try {
        await renderLoaded()
        searchFromMenu()
        act(() => { vi.advanceTimersByTime(REPO_SEARCH_DEBOUNCE_MS) })
        expect(repoSearch.onEvent).not.toBeNull()

        fireEvent.keyDown(screen.getByPlaceholderText('Search'), { key: 'Escape' })

        expect(screen.queryByPlaceholderText('Search')).toBeNull()
        expect(repoSearch.onEvent).toBeNull()
      } finally {
        vi.useRealTimers()
      }
    })
  })
})
