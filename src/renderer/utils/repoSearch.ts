import type { ExecApi } from '../types'
import { ExecEventType, type ExecEvent } from '../../shared/ipc-types'

// Searches the whole worktree with ripgrep (git grep when rg is not installed on the
// daemon host), streaming hits as the command prints them. Repos can be big, so the
// search is cancellable and capped.

export const MAX_REPO_HITS = 2000
// Long, because the caller cancels the search when it is no longer wanted.
export const REPO_SEARCH_TIMEOUT_MS = 5 * 60_000

export interface RepoHit {
  filePath: string
  lineNumber: number
  column: number
  preview: string
}

export enum RepoSearchTool {
  Ripgrep = 'rg',
  GitGrep = 'git grep',
}

export enum RepoSearchEventType {
  Hits = 'hits',
  Done = 'done',
  Error = 'error',
}

export type RepoSearchEvent =
  | { type: RepoSearchEventType.Hits; hits: RepoHit[] }
  | { type: RepoSearchEventType.Done; tool: RepoSearchTool; truncated: boolean }
  | { type: RepoSearchEventType.Error; message: string }

// `path:line:col:text`, the format of both `rg --vimgrep` and `git grep -n --column`.
const VIMGREP_LINE = /^(.+?):(\d+):(\d+):(.*)$/

export function parseVimgrepLine(line: string): RepoHit | Error {
  const match = VIMGREP_LINE.exec(line)
  if (!match) return new Error(`Unexpected search output: ${line}`)
  const [, rawPath = '', lineNumber = '', column = '', preview = ''] = match
  return {
    filePath: rawPath.startsWith('./') ? rawPath.slice(2) : rawPath,
    lineNumber: Number(lineNumber),
    column: Number(column),
    preview,
  }
}

// Same smart-case rule as the in-view matcher (createLineMatcher): an all-lowercase
// query matches case-insensitively.
function commandFor(tool: RepoSearchTool, query: string): { command: string; args: string[] } {
  if (tool === RepoSearchTool.Ripgrep) {
    return {
      command: 'rg',
      args: ['--vimgrep', '--fixed-strings', '--smart-case', '--max-columns', '300', '--max-columns-preview', '--', query, '.'],
    }
  }
  const caseArgs = query === query.toLowerCase() ? ['-i'] : []
  return { command: 'git', args: ['grep', '-n', '--column', '-I', '-F', ...caseArgs, '-e', query] }
}

// The daemon reports a binary it could not start this way (see exec_manager.rs).
function isSpawnError(message: string): boolean {
  return message.includes('spawn error')
}

/**
 * Start a repo-wide search for `query` under `cwd`. Events are delivered through
 * `onEvent` until a Done or Error event. The returned function cancels the search:
 * it kills the running command and no further events are delivered.
 */
export function startRepoSearch(
  execApi: ExecApi,
  connectionId: string,
  cwd: string,
  query: string,
  onEvent: (event: RepoSearchEvent) => void,
): () => void {
  let cancelled = false
  let execId: string | null = null
  let unsubscribe: () => void = () => {}
  let hitCount = 0

  const stop = (): void => {
    unsubscribe()
    if (execId !== null) execApi.kill(execId)
  }

  const run = (tool: RepoSearchTool): void => {
    const { command, args } = commandFor(tool, query)
    let buffer = ''
    let stderr = ''

    // Returns false once the cap is reached and the search was stopped.
    const emitLines = (lines: string[]): boolean => {
      const hits: RepoHit[] = []
      for (const line of lines) {
        if (!line) continue
        const hit = parseVimgrepLine(line)
        if (hit instanceof Error) {
          stop()
          onEvent({ type: RepoSearchEventType.Error, message: hit.message })
          return false
        }
        hits.push(hit)
        if (hitCount + hits.length >= MAX_REPO_HITS) break
      }
      hitCount += hits.length
      if (hits.length > 0) onEvent({ type: RepoSearchEventType.Hits, hits })
      if (hitCount >= MAX_REPO_HITS) {
        stop()
        onEvent({ type: RepoSearchEventType.Done, tool, truncated: true })
        return false
      }
      return true
    }

    const handle = (event: ExecEvent): void => {
      if (event.type === ExecEventType.Stdout) {
        const lines = (buffer + event.data).split('\n')
        buffer = lines.pop() ?? ''
        emitLines(lines)
      } else if (event.type === ExecEventType.Stderr) {
        stderr += event.data
      } else if (event.type === ExecEventType.Exit) {
        unsubscribe()
        execId = null
        if (!emitLines([buffer])) return
        // Both tools exit 1 when nothing matched.
        if (event.exitCode === 0 || event.exitCode === 1) {
          onEvent({ type: RepoSearchEventType.Done, tool, truncated: false })
        } else {
          onEvent({
            type: RepoSearchEventType.Error,
            message: `${command} exited with code ${String(event.exitCode)}: ${stderr.trim()}`,
          })
        }
      } else {
        unsubscribe()
        execId = null
        if (tool === RepoSearchTool.Ripgrep && isSpawnError(event.message)) {
          run(RepoSearchTool.GitGrep)
          return
        }
        onEvent({ type: RepoSearchEventType.Error, message: event.message })
      }
    }

    execId = null
    execApi.start(connectionId, cwd, command, args, REPO_SEARCH_TIMEOUT_MS).then((result) => {
      if (!result.success) {
        if (!cancelled) onEvent({ type: RepoSearchEventType.Error, message: result.error })
        return
      }
      execId = result.execId
      if (cancelled) {
        execApi.kill(result.execId)
        return
      }
      unsubscribe = execApi.onEvent(result.execId, (event) => {
        if (!cancelled) handle(event)
      })
    }).catch((error: unknown) => {
      if (!cancelled) onEvent({ type: RepoSearchEventType.Error, message: String(error) })
    })
  }

  run(RepoSearchTool.Ripgrep)

  return () => {
    cancelled = true
    stop()
  }
}

// Search hits are relative to the workspace; file tree entries (and so a file browser
// tab's selectedPath) are absolute.
export function toWorkspaceAbsolute(workspacePath: string, relativePath: string): string {
  return `${workspacePath}/${relativePath}`
}

export function toWorkspaceRelative(workspacePath: string, filePath: string): string {
  const prefix = `${workspacePath}/`
  return filePath.startsWith(prefix) ? filePath.slice(prefix.length) : filePath
}
