import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import { useStore } from 'zustand'
import { useAppStore } from '../store/app'
import { createRunActionsStore, type RunActionsState } from '../store/createRunActionsStore'
import type { RunAction } from '../types'
import type { StoreApi } from 'zustand'

interface RunActionPanelProps {
  workspacePath: string
  isVisible: boolean
}

interface RunExecution {
  actionId: string
  ptyId: string
  output: string
  status: 'running' | 'succeeded' | 'failed'
}

export default function RunActionPanel({ workspacePath, isVisible }: RunActionPanelProps) {
  const runActionsApi = useAppStore(s => s.runActions)
  const terminalApi = useAppStore(s => s.terminal)

  const store = useMemo<StoreApi<RunActionsState>>(() =>
    createRunActionsStore(workspacePath, {
      detect: runActionsApi.detect,
      run: runActionsApi.run
    }),
    [workspacePath, runActionsApi]
  )

  const actions = useStore(store, s => s.actions)
  const detecting = useStore(store, s => s.detecting)
  const run = useStore(store, s => s.run)

  const [executions, setExecutions] = useState<RunExecution[]>([])
  const [selectedExecIndex, setSelectedExecIndex] = useState<number | null>(null)
  const outputRef = useRef<HTMLPreElement>(null)

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

  const handleRun = useCallback(async (actionId: string) => {
    const ptyId = await run(actionId)
    if (!ptyId) return

    const execIndex = executions.length
    const newExec: RunExecution = {
      actionId,
      ptyId,
      output: '',
      status: 'running'
    }

    setExecutions(prev => [...prev, newExec])
    setSelectedExecIndex(execIndex)

    const unsubData = terminalApi.onData(ptyId, (data) => {
      setExecutions(prev => prev.map((e, i) =>
        i === execIndex ? { ...e, output: e.output + data } : e
      ))
    })

    const unsubExit = terminalApi.onExit(ptyId, (exitCode) => {
      setExecutions(prev => prev.map((e, i) =>
        i === execIndex ? { ...e, status: exitCode === 0 ? 'succeeded' : 'failed' } : e
      ))
      unsubData()
      unsubExit()
    })
  }, [run, terminalApi, executions.length])

  const handleKill = useCallback((exec: RunExecution) => {
    if (exec.status === 'running') {
      terminalApi.kill(exec.ptyId)
    }
  }, [terminalApi])

  // Auto-scroll output
  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight
    }
  }, [selectedExecIndex !== null ? executions[selectedExecIndex]?.output : null])

  if (!isVisible) return null

  const selectedExec = selectedExecIndex !== null ? executions[selectedExecIndex] : null

  return (
    <div className="run-action-panel">
      <div className="run-action-sidebar">
        <div className="run-action-header">
          <span>Run Actions</span>
          <button
            className="run-action-refresh-btn"
            onClick={() => store.getState().detect()}
            disabled={detecting}
            title="Refresh"
          >
            ↻
          </button>
        </div>
        {detecting && <div className="run-action-detecting">Detecting...</div>}
        {!detecting && actions.length === 0 && (
          <div className="run-action-empty">No actions found</div>
        )}
        {Array.from(grouped.entries()).map(([source, sourceActions]) => (
          <div key={source} className="run-action-group">
            <div className="run-action-group-header">{source}</div>
            {sourceActions.map(action => (
              <div key={action.id} className="run-action-item">
                <div className="run-action-item-info">
                  <span className="run-action-item-name">{action.name}</span>
                  {action.description && (
                    <span className="run-action-item-desc" title={action.description}>
                      {action.description}
                    </span>
                  )}
                </div>
                <button
                  className="run-action-run-btn"
                  onClick={() => handleRun(action.id)}
                  title={`Run ${action.name}`}
                >
                  ▶
                </button>
              </div>
            ))}
          </div>
        ))}

        {executions.length > 0 && (
          <>
            <div className="run-action-group-header" style={{ marginTop: '12px' }}>
              History
            </div>
            {executions.map((exec, i) => (
              <div
                key={i}
                className={`run-action-item ${selectedExecIndex === i ? 'selected' : ''}`}
                onClick={() => setSelectedExecIndex(i)}
              >
                <span className="run-action-item-name">
                  {exec.actionId.split(':').slice(1).join(':')}
                </span>
                <span className={`run-action-status run-action-status-${exec.status}`}>
                  {exec.status === 'running' ? '●' : exec.status === 'succeeded' ? '✓' : '✗'}
                </span>
              </div>
            ))}
          </>
        )}
      </div>

      <div className="run-action-output">
        {selectedExec ? (
          <>
            <div className="run-action-output-header">
              <span>{selectedExec.actionId}</span>
              {selectedExec.status === 'running' && (
                <button
                  className="run-action-kill-btn"
                  onClick={() => handleKill(selectedExec)}
                >
                  Stop
                </button>
              )}
              <span className={`run-action-status run-action-status-${selectedExec.status}`}>
                {selectedExec.status}
              </span>
            </div>
            <pre ref={outputRef} className="run-action-output-content">
              {selectedExec.output || '(waiting for output...)'}
            </pre>
          </>
        ) : (
          <div className="run-action-output-placeholder">
            Select an action to run, or click a history item to view output
          </div>
        )}
      </div>
    </div>
  )
}
