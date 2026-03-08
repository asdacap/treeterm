/**
 * Convert a keybinding to tinykeys format
 * Example: 'CommandOrControl+B' -> '$mod+b'
 */
export function convertDirectKeybinding(binding: string): string {
  return binding
    .replace('CommandOrControl', '$mod')
    .split('+')
    .map((part, index, arr) => {
      // Keep modifiers as-is, convert letter keys to lowercase
      if (index === arr.length - 1 && part.length === 1) {
        return part.toLowerCase()
      }
      return part
    })
    .join('+')
}
