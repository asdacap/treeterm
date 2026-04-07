import React, { useState } from 'react'
import type { DiffFile, UncommittedFile } from '../types'

interface TreeNode {
  name: string
  path: string
  children: TreeNode[]
  file: DiffFile | UncommittedFile | null
}

function buildTree(files: (DiffFile | UncommittedFile)[]): TreeNode[] {
  const root: TreeNode = { name: '', path: '', children: [], file: null }

  for (const file of files) {
    const parts = file.path.split('/')
    let current = root

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i]
      const isFile = i === parts.length - 1

      if (isFile) {
        current.children.push({
          name: part,
          path: file.path,
          children: [],
          file,
        })
      } else {
        let child = current.children.find((c) => c.file === null && c.name === part)
        if (!child) {
          child = {
            name: part,
            path: parts.slice(0, i + 1).join('/'),
            children: [],
            file: null,
          }
          current.children.push(child)
        }
        current = child
      }
    }
  }

  // Collapse single-child directory chains (e.g. src/ > renderer/ -> src/renderer/)
  function collapse(node: TreeNode): TreeNode {
    node.children = node.children.map(collapse)

    if (node.file === null && node.children.length === 1 && node.children[0].file === null) {
      const child = node.children[0]
      return {
        name: node.name + '/' + child.name,
        path: child.path,
        children: child.children,
        file: null,
      }
    }
    return node
  }

  // Sort: directories first, then files, both alphabetically
  function sort(nodes: TreeNode[]): TreeNode[] {
    return nodes.sort((a, b) => {
      const aIsDir = a.file === null
      const bIsDir = b.file === null
      if (aIsDir !== bIsDir) return aIsDir ? -1 : 1
      return a.name.localeCompare(b.name)
    }).map((node) => {
      if (node.file === null) {
        node.children = sort(node.children)
      }
      return node
    })
  }

  const collapsed = root.children.map(collapse)
  return sort(collapsed)
}

export function getSortedFilePaths(files: (DiffFile | UncommittedFile)[]): string[] {
  const tree = buildTree(files)
  const paths: string[] = []
  function walk(nodes: TreeNode[]) {
    for (const node of nodes) {
      if (node.file !== null) paths.push(node.path)
      else walk(node.children)
    }
  }
  walk(tree)
  return paths
}

function getAllDirPaths(nodes: TreeNode[]): Set<string> {
  const paths = new Set<string>()
  function walk(node: TreeNode) {
    if (node.file === null) {
      paths.add(node.path)
      node.children.forEach(walk)
    }
  }
  nodes.forEach(walk)
  return paths
}

// Committed files tree
interface CommittedTreeProps {
  files: DiffFile[]
  selectedFile: string | null
  onSelectFile: (path: string) => void
  getStatusIcon: (status: DiffFile['status']) => React.JSX.Element
}

export function CommittedDiffFileTree({
  files,
  selectedFile,
  onSelectFile,
  getStatusIcon,
}: CommittedTreeProps): React.JSX.Element {
  const tree = buildTree(files)
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(() => getAllDirPaths(tree))

  const toggleDir = (path: string) => {
    setExpandedDirs((prev) => {
      const next = new Set(prev)
      if (next.has(path)) {
        next.delete(path)
      } else {
        next.add(path)
      }
      return next
    })
  }

  function renderNode(node: TreeNode, depth: number): React.JSX.Element {
    if (node.file !== null) {
      const file = node.file as DiffFile
      return (
        <div
          key={node.path}
          className={`diff-file-item ${selectedFile === file.path ? 'selected' : ''}`}
          style={{ paddingLeft: `${String(depth * 16 + 12)}px` }}
          onClick={() => { onSelectFile(file.path); }}
          title={file.path}
        >
          {getStatusIcon(file.status)}
          <span className="diff-file-path">{node.name}</span>
          <span className="diff-file-stats">
            <span className="additions">+{file.additions}</span>
            <span className="deletions">-{file.deletions}</span>
          </span>
        </div>
      )
    }

    const isExpanded = expandedDirs.has(node.path)
    return (
      <div key={node.path}>
        <div
          className="diff-tree-dir"
          style={{ paddingLeft: `${String(depth * 16 + 12)}px` }}
          onClick={() => { toggleDir(node.path); }}
        >
          <span className="diff-tree-chevron">{isExpanded ? '\u25BC' : '\u25B6'}</span>
          <span className="diff-tree-dir-name">{node.name}</span>
        </div>
        {isExpanded && node.children.map((child) => renderNode(child, depth + 1))}
      </div>
    )
  }

  return <>{tree.map((node) => renderNode(node, 0))}</>
}

// Uncommitted files tree
interface UncommittedTreeProps {
  files: UncommittedFile[]
  selectedFile: UncommittedFile | null
  onSelectFile: (file: UncommittedFile) => void
  getStatusIcon: (status: UncommittedFile['status']) => React.JSX.Element
  onAction: (path: string) => void
  actionLabel: string
  stagingInProgress: boolean
}

export function UncommittedDiffFileTree({
  files,
  selectedFile,
  onSelectFile,
  getStatusIcon,
  onAction,
  actionLabel,
  stagingInProgress,
}: UncommittedTreeProps): React.JSX.Element {
  const tree = buildTree(files)
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(() => getAllDirPaths(tree))

  const toggleDir = (path: string) => {
    setExpandedDirs((prev) => {
      const next = new Set(prev)
      if (next.has(path)) {
        next.delete(path)
      } else {
        next.add(path)
      }
      return next
    })
  }

  function renderNode(node: TreeNode, depth: number): React.JSX.Element {
    if (node.file !== null) {
      const file = node.file as UncommittedFile
      return (
        <div
          key={node.path}
          className={`diff-file-item ${selectedFile?.path === file.path ? 'selected' : ''}`}
          style={{ paddingLeft: `${String(depth * 16 + 12)}px` }}
          onClick={() => { onSelectFile(file); }}
          title={file.path}
        >
          {getStatusIcon(file.status)}
          <span className="diff-file-path">{node.name}</span>
          <span className="diff-file-stats">
            <span className="additions">+{file.additions}</span>
            <span className="deletions">-{file.deletions}</span>
          </span>
          <button
            className="diff-file-action"
            onClick={(e) => {
              e.stopPropagation()
              onAction(file.path)
            }}
            disabled={stagingInProgress}
          >
            {actionLabel}
          </button>
        </div>
      )
    }

    const isExpanded = expandedDirs.has(node.path)
    return (
      <div key={node.path}>
        <div
          className="diff-tree-dir"
          style={{ paddingLeft: `${String(depth * 16 + 12)}px` }}
          onClick={() => { toggleDir(node.path); }}
        >
          <span className="diff-tree-chevron">{isExpanded ? '\u25BC' : '\u25B6'}</span>
          <span className="diff-tree-dir-name">{node.name}</span>
        </div>
        {isExpanded && node.children.map((child) => renderNode(child, depth + 1))}
      </div>
    )
  }

  return <>{tree.map((node) => renderNode(node, 0))}</>
}
