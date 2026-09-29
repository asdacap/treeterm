import React from 'react'

/** Auto-scrolls to bottom whenever children change */
export function AutoScrollPre({ className, children }: { className?: string; children: React.ReactNode }): React.JSX.Element {
  const ref = React.useRef<HTMLPreElement>(null)
  React.useEffect(() => {
    if (ref.current) {
      ref.current.scrollTop = ref.current.scrollHeight
    }
  })
  return <pre className={className} ref={ref}>{children}</pre>
}
