import type { ReactNode } from 'react'
import { Volume2, VolumeX, Loader2 } from 'lucide-react'
import { SoundSaveStatus, useSettingsStore } from '../store/settings'

export default function SoundToggle(): React.JSX.Element {
  const enabled = useSettingsStore(s => s.settings.notifications.soundEnabled)
  const saveState = useSettingsStore(s => s.soundSaveState)
  const toggleSound = useSettingsStore(s => s.toggleSound)
  const label = enabled ? 'Mute workspace notification sounds' : 'Enable workspace notification sounds'
  // Render callbacks are invoked directly, not mounted as component types.
  /* eslint-disable react/no-unstable-nested-components */
  const feedback: Record<SoundSaveStatus, () => ReactNode> = {
    [SoundSaveStatus.Idle]: () => null,
    [SoundSaveStatus.Saving]: () => <span role="status">Saving sound preference…</span>,
    [SoundSaveStatus.Error]: () => saveState.status === SoundSaveStatus.Error
      ? <span role="alert">Could not save sound preference: {saveState.error}</span> : null
  }
  /* eslint-enable react/no-unstable-nested-components */
  return (
    <span className="sound-toggle">
      <button
        type="button"
        className="add-button"
        aria-label={label}
        title={label}
        aria-pressed={enabled}
        disabled={saveState.status === SoundSaveStatus.Saving}
        onClick={() => { void toggleSound() }}
      >
        {saveState.status === SoundSaveStatus.Saving ? <Loader2 size={14} className="spinning" />
          : enabled ? <Volume2 size={14} /> : <VolumeX size={14} />}
      </button>
      {feedback[saveState.status]()}
    </span>
  )
}
