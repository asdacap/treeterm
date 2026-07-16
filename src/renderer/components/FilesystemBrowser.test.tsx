// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { describe, expect, it, vi } from 'vitest'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'
import type { WorkspaceStore } from '../types'
import { FilesystemBrowser } from './FilesystemBrowser'

vi.mock('./FileTree', () => ({ FileTree: () => <div data-testid="file-tree" /> }))
vi.mock('./FileViewer', () => ({ FileViewer: () => <div data-testid="file-viewer" /> }))
vi.mock('../hooks/useWorkspaceApis', () => ({
  useGitApi: () => ({ getHeadCommitHash: vi.fn().mockResolvedValue({ success: false }) }),
}))

function makeStore(): WorkspaceStore {
  const reviewComments = createStore(() => ({
    getReviewComments: () => [],
    addReviewComment: vi.fn(),
    deleteReviewComment: vi.fn(),
    updateOutdatedReviewComments: vi.fn(),
  }))
  const store = createStore(() => ({
    workspace: makeWorkspace({
      appStates: { 'tab-1': { applicationId: 'filesystem', title: 'Files', state: { selectedPath: null, expandedDirs: [] } } },
    }),
    updateTabState: vi.fn(),
    reviewComments,
  }))
  return store as unknown as WorkspaceStore
}

describe('FilesystemBrowser tree collapse', () => {
  it('hides the file tree when collapsed and restores it when expanded', () => {
    render(<FilesystemBrowser workspace={makeStore()} tabId="tab-1" />)

    // Expanded by default: tree visible, collapse control shown.
    expect(screen.getByTestId('file-tree')).toBeDefined()
    const collapse = screen.getByRole('button', { name: 'Hide file tree' })

    fireEvent.click(collapse)

    // Collapsed: tree gone, expand control shown.
    expect(screen.queryByTestId('file-tree')).toBeNull()
    const expand = screen.getByRole('button', { name: 'Show file tree' })

    fireEvent.click(expand)

    // Expanded again.
    expect(screen.getByTestId('file-tree')).toBeDefined()
    expect(screen.getByRole('button', { name: 'Hide file tree' })).toBeDefined()
  })
})
