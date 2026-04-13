import React, { useState } from 'react'
import { MultiFileDiff, WorkerPoolContextProvider } from '@pierre/diffs/react'
import type { DiffLineAnnotation, OnDiffLineClickProps, AnnotationSide } from '@pierre/diffs'
import { ChevronLeft, ChevronRight, Columns2, AlignJustify, Eye, EyeOff, MessageSquare } from 'lucide-react'
import type { ReviewComment } from '../types'
import { CommentInput } from './CommentInput'
import { CommentDisplay } from './CommentDisplay'
import { createDiffsWorker } from '../pierre-diffs-config'

interface PierreDiffViewerProps {
  originalContent: string
  modifiedContent: string
  filePath: string
  originalLabel: string
  modifiedLabel: string
  onPreviousFile?: () => void
  onNextFile?: () => void
  hasPreviousFile: boolean
  hasNextFile: boolean
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
  originalLabel,
  modifiedLabel,
  onPreviousFile,
  onNextFile,
  hasPreviousFile,
  hasNextFile,
  comments,
  onLineClick,
  inlineCommentInput,
  onCommentSubmit,
  onCommentCancel,
  onCommentDelete,
}: PierreDiffViewerProps): React.JSX.Element {
  const [isSplitView, setIsSplitView] = useState(true)
  const [hideUnchangedRegions, setHideUnchangedRegions] = useState(false)

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
    <WorkerPoolContextProvider
      poolOptions={{ workerFactory: createDiffsWorker, poolSize: 2 }}
      highlighterOptions={{ preferredHighlighter: 'shiki-wasm' }}
    >
      <div className="pierre-diff-wrapper">
        <div className="pierre-diff-toolbar">
          <div className="pierre-diff-labels">
            <span className="pierre-diff-label">{originalLabel}</span>
            <span className="pierre-diff-arrow">→</span>
            <span className="pierre-diff-label">{modifiedLabel}</span>
          </div>

          <div className="pierre-diff-controls">
            {onPreviousFile && (
              <button
                className="pierre-diff-btn"
                onClick={onPreviousFile}
                disabled={!hasPreviousFile}
                title="Previous file"
              >
                <ChevronLeft size={14} />
              </button>
            )}
            {onNextFile && (
              <button
                className="pierre-diff-btn"
                onClick={onNextFile}
                disabled={!hasNextFile}
                title="Next file"
              >
                <ChevronRight size={14} />
              </button>
            )}

            <button
              className={`pierre-diff-btn ${isSplitView ? 'active' : ''}`}
              onClick={() => { setIsSplitView(!isSplitView) }}
              title={isSplitView ? 'Switch to unified view' : 'Switch to split view'}
            >
              {isSplitView ? <Columns2 size={14} /> : <AlignJustify size={14} />}
            </button>

            <button
              className={`pierre-diff-btn ${hideUnchangedRegions ? 'active' : ''}`}
              onClick={() => { setHideUnchangedRegions(!hideUnchangedRegions) }}
              title={hideUnchangedRegions ? 'Show unchanged regions' : 'Hide unchanged regions'}
            >
              {hideUnchangedRegions ? <EyeOff size={14} /> : <Eye size={14} />}
            </button>

            {comments.length > 0 && (
              <span className="pierre-diff-comment-count" title={`${String(comments.length)} comment(s)`}>
                <MessageSquare size={14} />
                {comments.length}
              </span>
            )}
          </div>
        </div>

        <div className="pierre-diff-content">
          <MultiFileDiff<CommentAnnotationData>
            oldFile={{ name: filePath, contents: originalContent }}
            newFile={{ name: filePath, contents: modifiedContent }}
            lineAnnotations={lineAnnotations}
            renderAnnotation={renderAnnotation}
            options={{
              diffStyle: isSplitView ? 'split' : 'unified',
              expandUnchanged: !hideUnchangedRegions,
              theme: 'treeterm-dark',
              themeType: 'dark',
              disableFileHeader: true,
              overflow: 'wrap',
              onLineNumberClick: handleLineNumberClick,
            }}
            style={{ height: '100%', width: '100%' }}
          />
        </div>
      </div>
    </WorkerPoolContextProvider>
  )
}
