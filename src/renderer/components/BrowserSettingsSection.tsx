import type { BrowserInstance } from '../types'
import { parseLocalUrl } from '../../applications/browser/url'

interface BrowserSettingsSectionProps {
  instances: BrowserInstance[]
  onChange: (instances: BrowserInstance[]) => void
}

function urlProblem(url: string): string {
  try {
    parseLocalUrl(url)
    return ''
  } catch (err) {
    return err instanceof Error ? err.message : String(err)
  }
}

export default function BrowserSettingsSection({ instances, onChange }: BrowserSettingsSectionProps) {
  const updateAt = (index: number, patch: Partial<BrowserInstance>): void => {
    onChange(instances.map((inst, i) => (i === index ? { ...inst, ...patch } : inst)))
  }

  return (
    <div className="settings-section">
      <p className="settings-hint">
        Open a page that is already being served, such as a dashboard. The URL must be on
        localhost; on remote sessions its port is forwarded over SSH first.
      </p>
      <div className="applications-list">
        {instances.map((inst, index) => {
          const problem = urlProblem(inst.url)
          return (
            <div key={inst.id} className="application-item">
              <div className="application-icon">
                <input
                  type="text"
                  className="settings-input icon-input"
                  value={inst.icon}
                  maxLength={2}
                  onChange={(e) => { updateAt(index, { icon: e.target.value }) }}
                />
              </div>
              <div className="application-fields">
                <input
                  type="text"
                  className="settings-input"
                  value={inst.name}
                  placeholder="Name (e.g., Grafana)"
                  onChange={(e) => { updateAt(index, { name: e.target.value }) }}
                />
                <input
                  type="text"
                  className="settings-input"
                  value={inst.url}
                  placeholder="URL (e.g., http://localhost:8080/)"
                  onChange={(e) => { updateAt(index, { url: e.target.value }) }}
                />
                {problem && <div className="settings-hint webapp-error">{problem}</div>}
              </div>
              <label className="settings-checkbox-label default-checkbox">
                <input
                  type="checkbox"
                  checked={inst.isDefault}
                  onChange={(e) => { updateAt(index, { isDefault: e.target.checked }) }}
                />
                Default
              </label>
              <button
                className="application-delete"
                onClick={() => { onChange(instances.filter((_, i) => i !== index)) }}
              >
                Delete
              </button>
            </div>
          )
        })}
      </div>
      <button
        className="settings-btn add-app"
        onClick={() => {
          onChange([
            ...instances,
            {
              id: `browser-${String(Date.now())}`,
              name: 'New Browser',
              icon: '🧭',
              url: 'http://localhost:8080/',
              isDefault: false,
            },
          ])
        }}
      >
        + Add Browser
      </button>
      <p className="settings-hint">
        Default browsers open automatically in new workspaces.
      </p>
    </div>
  )
}
