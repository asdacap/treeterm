import { usePrefixModeStore } from '../store/prefixMode'
import { useSettingsStore } from '../store/settings'

function formatPrefixKey(key: string): string {
  return key
    .replace('CommandOrControl', window.electron.platform === 'darwin' ? 'Cmd' : 'Ctrl')
    .replace('Control', 'Ctrl')
    .replace(/\+/g, ' + ')
}

export default function PrefixModeIndicator() {
  const { state } = usePrefixModeStore()
  const { settings } = useSettingsStore()

  if (!settings.prefixMode.enabled || state !== 'active') {
    return null
  }

  return (
    <div className="prefix-mode-indicator">
      <span className="prefix-mode-key">{formatPrefixKey(settings.prefixMode.prefixKey)}</span>
      <span className="prefix-mode-label">Prefix Mode</span>
    </div>
  )
}
