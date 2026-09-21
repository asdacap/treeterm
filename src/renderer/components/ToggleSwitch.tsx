interface ToggleSwitchProps {
  checked: boolean
  onChange: (checked: boolean) => void
  label: string
}

/** The small labelled slider used in terminal status bars. */
export function ToggleSwitch({ checked, onChange, label }: ToggleSwitchProps) {
  return (
    <label className="terminal-toggle">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => { onChange(e.target.checked) }}
      />
      <span className="terminal-toggle-slider" />
      <span className="terminal-toggle-label">{label}</span>
    </label>
  )
}
