import React, { useEffect, useRef } from 'react'
import { ChevronDown, ChevronUp, Search, X } from 'lucide-react'

interface DiffFindBarProps {
  query: string
  onQueryChange: (query: string) => void
  onNext: () => void
  onPrev: () => void
  onClose: () => void
  // 1-based position of the current match, or null when none is selected.
  current: number | null
  total: number
  // Some files are still loading, so the total may grow.
  pending: boolean
  // Bumped on every Cmd/Ctrl+F so a repeat press refocuses the input.
  focusNonce: number
}

function countLabel(query: string, current: number | null, total: number, pending: boolean): string {
  if (!query) return ''
  if (pending && total === 0) return 'Searching…'
  const count = current === null ? `${String(total)} found` : `${String(current)}/${String(total)}`
  return pending ? `${count}…` : count
}

export function DiffFindBar({
  query,
  onQueryChange,
  onNext,
  onPrev,
  onClose,
  current,
  total,
  pending,
  focusNonce,
}: DiffFindBarProps): React.JSX.Element {
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [focusNonce])

  return (
    <div className="diff-find-bar">
      <Search size={12} className="diff-find-icon" />
      <input
        ref={inputRef}
        className="diff-find-input"
        placeholder="Find in diff"
        value={query}
        onChange={(e) => { onQueryChange(e.target.value) }}
        onKeyDown={(e) => {
          // eslint-disable-next-line custom/no-string-literal-comparison -- DOM key names are external
          if (e.key === 'Enter') {
            e.preventDefault()
            if (e.shiftKey) onPrev()
            else onNext()
          // eslint-disable-next-line custom/no-string-literal-comparison -- DOM key names are external
          } else if (e.key === 'Escape') {
            e.preventDefault()
            onClose()
          }
        }}
      />
      <span className="diff-find-count">{countLabel(query, current, total, pending)}</span>
      <button className="diff-find-btn" onClick={onPrev} disabled={total === 0} title="Previous match (Shift+Enter)">
        <ChevronUp size={14} />
      </button>
      <button className="diff-find-btn" onClick={onNext} disabled={total === 0} title="Next match (Enter)">
        <ChevronDown size={14} />
      </button>
      <button className="diff-find-btn" onClick={onClose} title="Close (Escape)">
        <X size={14} />
      </button>
    </div>
  )
}
