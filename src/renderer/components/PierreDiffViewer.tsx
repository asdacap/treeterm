import React from 'react'
import { MultiFileDiff } from '@pierre/diffs/react'
import type { DiffLineAnnotation, OnDiffLineClickProps, AnnotationSide } from '@pierre/diffs'
import type { ReviewComment } from '../types'
import { CommentInput } from './CommentInput'
import { CommentDisplay } from './CommentDisplay'

interface PierreDiffViewerProps {
  originalContent: string
  modifiedContent: string
  filePath: string
  diffStyle: 'split' | 'unified'
  expandUnchanged: boolean
  comments: ReviewComment[]
  onLineClick?: (lineNumber: number, side: 'original' | 'modified') => void
  inlineCommentInput: { lineNumber: number; side: 'original' | 'modified' } | null
  onCommentSubmit?: (text: string) => void
  onCommentCancel?: () => void
  onCommentDelete?: (commentId: string) => void
}

type CommentAnnotationData = {
  comments: ReviewComment[]
  isInput: boolean
  inputSide?: 'original' | 'modified'
}

function sideToAnnotation(side: 'original' | 'modified'): AnnotationSide {
  return side === 'original' ? 'deletions' : 'additions'
}

function annotationToSide(side: AnnotationSide): 'original' | 'modified' {
  return side === 'deletions' ? 'original' : 'modified'
}

export function PierreDiffViewer({
  originalContent,
  modifiedContent,
  filePath,
  diffStyle,
  expandUnchanged,
  comments,
  onLineClick,
  inlineCommentInput,
  onCommentSubmit,
  onCommentCancel,
  onCommentDelete,
}: PierreDiffViewerProps): React.JSX.Element {
  const handleLineNumberClick = (props: OnDiffLineClickProps) => {
    onLineClick?.(props.lineNumber, annotationToSide(props.annotationSide))
  }

  const lineAnnotations: DiffLineAnnotation<CommentAnnotationData>[] = []

  // Group comments by line+side
  const commentGroups = new Map<string, ReviewComment[]>()
  for (const comment of comments) {
    const key = `${String(comment.lineNumber)}-${comment.side}`
    const group = commentGroups.get(key)
    if (group) {
      group.push(comment)
    } else {
      commentGroups.set(key, [comment])
    }
  }

  commentGroups.forEach((groupComments, key) => {
    const [lineStr, side] = key.split('-') as [string, 'original' | 'modified']
    lineAnnotations.push({
      side: sideToAnnotation(side),
      lineNumber: Number(lineStr),
      metadata: { comments: groupComments, isInput: false },
    })
  })

  // Add annotation for comment input
  if (inlineCommentInput) {
    const existingIdx = lineAnnotations.findIndex(
      a => a.lineNumber === inlineCommentInput.lineNumber &&
           a.side === sideToAnnotation(inlineCommentInput.side)
    )
    const existing = lineAnnotations[existingIdx]
    if (existingIdx >= 0 && existing) {
      lineAnnotations[existingIdx] = {
        side: existing.side,
        lineNumber: existing.lineNumber,
        metadata: { ...existing.metadata, isInput: true, inputSide: inlineCommentInput.side },
      }
    } else {
      lineAnnotations.push({
        side: sideToAnnotation(inlineCommentInput.side),
        lineNumber: inlineCommentInput.lineNumber,
        metadata: { comments: [], isInput: true, inputSide: inlineCommentInput.side },
      })
    }
  }

  const renderAnnotation = (annotation: DiffLineAnnotation<CommentAnnotationData>): React.ReactNode => {
    const { metadata } = annotation
    return (
      <div className="pierre-diff-annotation">
        {metadata.comments.map(comment => (
          <CommentDisplay
            key={comment.id}
            comment={comment}
            onDelete={(id) => { onCommentDelete?.(id) }}
            hideLineRef
          />
        ))}
        {metadata.isInput && onCommentSubmit && onCommentCancel && (
          <CommentInput
            lineNumber={annotation.lineNumber}
            side={metadata.inputSide}
            onSubmit={onCommentSubmit}
            onCancel={onCommentCancel}
          />
        )}
      </div>
    )
  }

  return (
    <div className="pierre-diff-content">
      <MultiFileDiff<CommentAnnotationData>
        oldFile={{ name: filePath, contents: originalContent }}
        newFile={{ name: filePath, contents: modifiedContent }}
        lineAnnotations={lineAnnotations}
        renderAnnotation={renderAnnotation}
        options={{
          diffStyle,
          expandUnchanged,
          theme: 'treeterm-dark',
          themeType: 'dark',
          disableFileHeader: true,
          overflow: 'wrap',
          onLineNumberClick: handleLineNumberClick,
        }}
        style={{ height: '100%', width: '100%' }}
      />
    </div>
  )
}
