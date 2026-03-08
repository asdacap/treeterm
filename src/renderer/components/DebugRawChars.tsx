interface DebugRawCharsProps {
  rawChars: string
}

// Convert raw characters to visible representation
function formatRawChars(str: string): string {
  let result = ''
  for (let i = 0; i < str.length; i++) {
    const code = str.charCodeAt(i)
    if (code === 0x1b) {
      result += '\\x1b'
    } else if (code === 0x0d) {
      result += '\\r'
    } else if (code === 0x0a) {
      result += '\\n'
    } else if (code === 0x09) {
      result += '\\t'
    } else if (code < 0x20) {
      result += `\\x${code.toString(16).padStart(2, '0')}`
    } else {
      result += str[i]
    }
  }
  return result
}

export default function DebugRawChars({ rawChars }: DebugRawCharsProps) {
  if (!rawChars) return null

  return (
    <div className="debug-raw-chars">
      <span className="debug-raw-chars-label">RAW:</span>
      <span className="debug-raw-chars-content">{formatRawChars(rawChars)}</span>
    </div>
  )
}
