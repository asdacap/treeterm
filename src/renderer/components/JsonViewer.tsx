import React, { useState } from 'react'

function JsonValue({ value, depth, defaultExpanded }: { value: unknown; depth: number; defaultExpanded: boolean }): React.JSX.Element {
  if (value === null) {
    return <span className="json-viewer-null">null</span>
  }
  if (typeof value === 'string') {
    return <span className="json-viewer-string">&quot;{value}&quot;</span>
  }
  if (typeof value === 'number') {
    return <span className="json-viewer-number">{String(value)}</span>
  }
  if (typeof value === 'boolean') {
    return <span className="json-viewer-boolean">{String(value)}</span>
  }
  if (Array.isArray(value)) {
    return <JsonCollection entries={value.map((v, i) => [String(i), v])} kind="array" depth={depth} defaultExpanded={defaultExpanded} />
  }
  if (typeof value === 'object') {
    return <JsonCollection entries={Object.entries(value as Record<string, unknown>)} kind="object" depth={depth} defaultExpanded={defaultExpanded} />
  }
  return <span>{JSON.stringify(value)}</span>
}

function JsonCollection({
  entries,
  kind,
  depth,
  defaultExpanded,
}: {
  entries: [string, unknown][]
  kind: 'object' | 'array'
  depth: number
  defaultExpanded: boolean
}): React.JSX.Element {
  const [expanded, setExpanded] = useState(defaultExpanded)
  const open = kind === 'object' ? '{' : '['
  const close = kind === 'object' ? '}' : ']'
  const count = entries.length

  if (count === 0) {
    return <span className="json-viewer-bracket">{open}{close}</span>
  }

  if (!expanded) {
    return (
      <span>
        <button className="json-viewer-toggle" onClick={() => { setExpanded(true); }}>&#x25b8;</button>
        <span className="json-viewer-bracket">{open}</span>
        <span className="json-viewer-summary" onClick={() => { setExpanded(true); }}>
          {count} {count === 1 ? 'item' : 'items'}
        </span>
        <span className="json-viewer-bracket">{close}</span>
      </span>
    )
  }

  return (
    <span>
      <button className="json-viewer-toggle" onClick={() => { setExpanded(false); }}>&#x25be;</button>
      <span className="json-viewer-bracket">{open}</span>
      <div className="json-viewer-children">
        {entries.map(([key, val], i) => (
          <div key={key} className="json-viewer-row">
            {kind === 'object' ? (
              <span className="json-viewer-key">&quot;{key}&quot;</span>
            ) : (
              <span className="json-viewer-index">{key}</span>
            )}
            <span className="json-viewer-colon">: </span>
            <JsonValue value={val} depth={depth + 1} defaultExpanded={depth + 1 < 1} />
            {i < entries.length - 1 && <span className="json-viewer-comma">,</span>}
          </div>
        ))}
      </div>
      <span className="json-viewer-bracket">{close}</span>
    </span>
  )
}

export default function JsonViewer({ data }: { data: unknown }): React.JSX.Element {
  return (
    <div className="json-viewer">
      <JsonValue value={data} depth={0} defaultExpanded={true} />
    </div>
  )
}
