import type { ReviewComment } from '../types'

/**
 * Format a markdown prompt for the given comments, regardless of their
 * addressed/prompted state. Used for re-prompting a single comment.
 */
export function buildPromptForComments(comments: ReviewComment[]): string {
  if (comments.length === 0) return ''

  // Group by filePath
  const grouped = new Map<string, ReviewComment[]>()
  for (const comment of comments) {
    const existing = grouped.get(comment.filePath) || []
    existing.push(comment)
    grouped.set(comment.filePath, existing)
  }

  const lines: string[] = ['Please address the following review comments:', '']

  for (const [filePath, fileComments] of Array.from(grouped.entries())) {
    lines.push(`## ${filePath}`)
    for (const c of fileComments) {
      const outdated = c.isOutdated ? ' [OUTDATED]' : ''
      lines.push(`- Line ${String(c.lineNumber)} (${c.side}): "${c.text}"${outdated}`)
    }
    lines.push('')
  }

  return lines.join('\n')
}

export function generateReviewPrompt(comments: ReviewComment[]): string {
  return buildPromptForComments(comments.filter(c => !c.addressed))
}
