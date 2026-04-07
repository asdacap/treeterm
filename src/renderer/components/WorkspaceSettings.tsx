import { useState } from 'react'
import { useStore } from 'zustand'
import type { Application, WorkspaceStore } from '../types'

interface WorkspaceSettingsProps {
  workspace: WorkspaceStore
  applications: Record<string, Application>
}

export default function WorkspaceSettings({ workspace, applications }: WorkspaceSettingsProps) {
  const { workspace: ws, updateMetadata, updateSettings } = useStore(workspace)
  const appList = Object.values(applications).filter((app) => app.showInNewTabMenu)

  const [name, setName] = useState(ws.metadata.displayName || ws.name)
  const [description, setDescription] = useState(ws.metadata.description || '')
  const [jsonExpanded, setJsonExpanded] = useState(false)

  const handleNameBlur = () => {
    const trimmed = name.trim()
    if (trimmed) {
      updateMetadata('displayName', trimmed)
    }
  }

  const handleDescriptionBlur = () => {
    updateMetadata('description', description.trim())
  }

  const handleDefaultAppChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    updateSettings({ defaultApplicationId: e.target.value || '' })
  }

  return (
    <div className="workspace-settings">
      <h2>Workspace Settings</h2>

      <div className="settings-group">
        <label className="settings-label">Name</label>
        <input
          className="settings-input"
          type="text"
          value={name}
          onChange={(e) => { setName(e.target.value); }}
          onBlur={handleNameBlur}
          onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur() }}
        />
      </div>

      <div className="settings-group">
        <label className="settings-label">Description</label>
        <textarea
          className="settings-input"
          value={description}
          onChange={(e) => { setDescription(e.target.value); }}
          onBlur={handleDescriptionBlur}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              ;(e.target as HTMLTextAreaElement).blur()
            }
          }}
          placeholder="Add a description..."
          rows={3}
        />
      </div>

      <div className="settings-group">
        <label className="settings-label">Default Application for Children</label>
        <select
          className="settings-select"
          value={ws.settings.defaultApplicationId}
          onChange={handleDefaultAppChange}
        >
          <option value="">Use Global Default</option>
          <option disabled>──────────</option>
          {appList.map((app) => (
            <option key={app.id} value={app.id}>{app.name}</option>
          ))}
        </select>
        <span className="settings-hint">
          The default application to open when creating child workspaces. If not set, inherits from parent or uses the global default.
        </span>
      </div>

      <div className="settings-group">
        <div
          className="workspace-settings-json-toggle"
          onClick={() => { setJsonExpanded(!jsonExpanded); }}
        >
          <span>{jsonExpanded ? '▾' : '▸'}</span>
          Raw Workspace JSON
        </div>
        {jsonExpanded && (
          <div className="workspace-settings-json">
            <pre>{JSON.stringify(ws, null, 2)}</pre>
          </div>
        )}
      </div>
    </div>
  )
}
