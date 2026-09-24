import type { WebAppInstance } from '../types'

interface WebAppSettingsSectionProps {
  instances: WebAppInstance[]
  onChange: (instances: WebAppInstance[]) => void
}

export default function WebAppSettingsSection({ instances, onChange }: WebAppSettingsSectionProps) {
  const updateAt = (index: number, patch: Partial<WebAppInstance>): void => {
    onChange(instances.map((inst, i) => (i === index ? { ...inst, ...patch } : inst)))
  }

  return (
    <div className="settings-section">
      <p className="settings-hint">
        Configure web apps such as dev servers. Use $PORT in the command; it is replaced with a
        free port, and the Browser tab opens that port once the server responds.
        On remote sessions the port is forwarded over SSH first.
      </p>
      <div className="applications-list">
        {instances.map((inst, index) => (
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
                placeholder="Name (e.g., Next.js)"
                onChange={(e) => { updateAt(index, { name: e.target.value }) }}
              />
              <input
                type="text"
                className="settings-input"
                value={inst.command}
                placeholder="Command (e.g., npm run dev -- -p $PORT)"
                onChange={(e) => { updateAt(index, { command: e.target.value }) }}
              />
              <label className="settings-checkbox-label">
                <input
                  type="checkbox"
                  checked={inst.keepOnExit}
                  onChange={(e) => { updateAt(index, { keepOnExit: e.target.checked }) }}
                />
                Keep Tab Open on Exit
              </label>
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
        ))}
      </div>
      <button
        className="settings-btn add-app"
        onClick={() => {
          onChange([
            ...instances,
            {
              id: `webapp-${String(Date.now())}`,
              name: 'New Web App',
              icon: '🌐',
              command: 'npm run dev -- -p $PORT',
              isDefault: false,
              keepOnExit: true,
            },
          ])
        }}
      >
        + Add Web App
      </button>
      <p className="settings-hint">
        Default web apps open automatically in new workspaces.
      </p>
    </div>
  )
}
