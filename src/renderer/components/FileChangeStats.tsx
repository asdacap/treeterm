import React from 'react'
import { FileStatKind, type FileChangeStats as FileChangeStatsType } from '../types'

export function FileChangeStats({ stats }: { stats: FileChangeStatsType }): React.JSX.Element {
  if (stats.kind === FileStatKind.Binary) {
    const change = stats.byteChange
    return <span className={change > 0 ? 'additions' : change < 0 ? 'deletions' : 'binary-bytes'}>{change > 0 ? '+' : ''}{change} bytes</span>
  }

  return (
    <>
      <span className="additions">+{stats.additions}</span>
      <span className="deletions">-{stats.deletions}</span>
    </>
  )
}
