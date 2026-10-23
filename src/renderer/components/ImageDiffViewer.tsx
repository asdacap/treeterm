import React from 'react'
import type { ImageDiffContents } from '../types'
import { imageMimeType } from '../../shared/fileContentKind'

interface ImageDiffViewerProps {
  contents: ImageDiffContents
  filePath: string
}

function ImageDiffSide({ label, base64, filePath }: { label: string; base64: string; filePath: string }): React.JSX.Element {
  return (
    <div className="image-diff-side">
      <div className="image-diff-label">{label}</div>
      {base64 ? (
        <img src={`data:${imageMimeType(filePath)};base64,${base64}`} alt={`${label}: ${filePath}`} />
      ) : (
        <div className="image-diff-absent">No file</div>
      )}
    </div>
  )
}

export function ImageDiffViewer({ contents, filePath }: ImageDiffViewerProps): React.JSX.Element {
  return (
    <div className="image-diff">
      <ImageDiffSide label="Before" base64={contents.originalBase64} filePath={filePath} />
      <ImageDiffSide label="After" base64={contents.modifiedBase64} filePath={filePath} />
    </div>
  )
}
