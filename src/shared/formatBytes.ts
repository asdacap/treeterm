/** A byte count in megabytes to one decimal place, e.g. `1.4MB`. */
export function formatMegabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
}
