import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import { ChevronDown } from 'lucide-react'
import { useStore } from 'zustand'
import { createRunActionsStore } from '../store/createRunActionsStore'
import type { RunAction, RunActionsApi } from '../types'

interface RunActionDropdownProps {
  workspacePath: string
  runActions: RunActionsApi
  onRun: (ptyId: string, actionId: string) => void
}

export default function RunActionDropdown({ workspacePath, runActions: runActionsApi, onRun }: RunActionDropdownProps) {
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)

  const store = useMemo(() =>
    createRunActionsStore(workspacePath, {
      detect: runActionsApi.detect,
      run: runActionsApi.run
    }),
    [workspacePath, runActionsApi]
  )

  const actions = useStore(store, s => s.actions)
  const detecting = useStore(store, s => s.detecting)

  // Group actions by source
  const grouped = useMemo(() => {
    const map = new Map<string, RunAction[]>()
    for (const action of actions) {
      const group = map.get(action.source) || []
      group.push(action)
      map.set(action.source, group)
    }
    return map
  }, [actions])

  // Close menu on click outside
  useEffect(() => {
    if (!menuOpen) return
    const handleClickOutside = (e: MouseEvent) => {
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        buttonRef.current && !buttonRef.current.contains(e.target as Node)
      ) {
        setMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [menuOpen])

  const handleRun = useCallback(async (actionId: string) => {
    setMenuOpen(false)
    const result = await runActionsApi.run(workspacePath, actionId)
    if (result.success) {
      onRun(result.ptyId, actionId)
    }
  }, [runActionsApi, workspacePath, onRun])

  return (
    <div className="run-action-dropdown-container">
      <button
        ref={buttonRef}
        className="workspace-action-btn run-action-trigger-btn"
        onClick={() => setMenuOpen(!menuOpen)}
        title="Run action"
      >
        ▶ Run <ChevronDown size={12} />
      </button>
      {menuOpen && (
        <div className="run-action-menu" ref={menuRef}>
          {detecting && (
            <div className="run-action-menu-loading">Loading...</div>
          )}
          {!detecting && actions.length === 0 && (
            <div className="run-action-menu-empty">No actions found</div>
          )}
          {!detecting && Array.from(grouped.entries()).map(([source, sourceActions]) => (
            <div key={source} className="run-action-menu-group">
              <div className="run-action-menu-group-header">{source}</div>
              {sourceActions.map(action => (
                <div
                  key={action.id}
                  className="run-action-menu-item"
                  onClick={() => handleRun(action.id)}
                  title={action.description}
                >
                  {action.name}
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
