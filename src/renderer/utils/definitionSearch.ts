import type { ExecApi, ExecEvent } from '../types'

export interface DefinitionLocation {
  filePath: string
  lineNumber: number
}

/** Language-specific regex patterns for symbol definitions */
function getDefinitionPattern(symbol: string, language: string): string {
  const escaped = symbol.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

  switch (language) {
    case 'typescript':
    case 'javascript':
      return `(function|const|let|var|class|interface|type|enum)\\s+${escaped}\\b`
    case 'python':
      return `(def|class)\\s+${escaped}\\b`
    case 'rust':
      return `(fn|struct|enum|trait|type|const|static|mod|impl)\\s+${escaped}\\b`
    case 'go':
      return `(func|type|var|const)\\s+${escaped}\\b`
    case 'java':
    case 'kotlin':
      return `(class|interface|enum|void|public|private|protected)\\s+.*${escaped}\\b`
    case 'ruby':
      return `(def|class|module)\\s+${escaped}\\b`
    case 'php':
      return `(function|class|interface|trait)\\s+${escaped}\\b`
    default:
      return `(function|class|def|fn|type|interface|struct|enum)\\s+${escaped}\\b`
  }
}

/** File globs for grep --include by language */
function getFileGlobs(language: string): string[] {
  switch (language) {
    case 'typescript':
      return ['*.ts', '*.tsx']
    case 'javascript':
      return ['*.js', '*.jsx', '*.mjs', '*.cjs']
    case 'python':
      return ['*.py']
    case 'rust':
      return ['*.rs']
    case 'go':
      return ['*.go']
    case 'java':
      return ['*.java']
    case 'kotlin':
      return ['*.kt', '*.kts']
    case 'ruby':
      return ['*.rb']
    case 'php':
      return ['*.php']
    case 'css':
    case 'scss':
    case 'less':
      return ['*.css', '*.scss', '*.less']
    case 'html':
      return ['*.html', '*.htm']
    default:
      return []
  }
}

/** Parse grep output lines (file:line:content) into DefinitionLocation[] */
function parseGrepOutput(stdout: string): DefinitionLocation[] {
  const results: DefinitionLocation[] = []
  for (const line of stdout.split('\n')) {
    if (!line.trim()) continue
    // grep -rn output: file:lineNumber:content
    const match = line.match(/^(.+?):(\d+):/)
    if (match) {
      const filePath = match[1]!.startsWith('./') ? match[1]!.slice(2) : match[1]!
      results.push({
        filePath,
        lineNumber: parseInt(match[2]!, 10),
      })
    }
  }
  return results
}

/**
 * Search for symbol definitions using grep via the streaming exec API.
 * Returns a Promise that resolves when the grep command completes.
 */
export function searchDefinition(
  execApi: ExecApi,
  connectionId: string,
  workspacePath: string,
  symbol: string,
  language: string,
): Promise<DefinitionLocation[]> {
  const pattern = getDefinitionPattern(symbol, language)
  const globs = getFileGlobs(language)

  const args = ['-rnE', pattern]
  for (const glob of globs) {
    args.push(`--include=${glob}`)
  }
  // Exclude common non-source directories
  args.push('--exclude-dir=node_modules', '--exclude-dir=.git', '--exclude-dir=dist', '--exclude-dir=build', '--exclude-dir=target')
  args.push('.')

  return new Promise((resolve) => {
    let stdout = ''

    execApi.start(connectionId, workspacePath, 'grep', args).then((result) => {
      if (!result.success) {
        resolve([])
        return
      }

      const unsubscribe = execApi.onEvent(result.execId, (event: ExecEvent) => {
        if (event.type === 'stdout') {
          stdout += event.data
        } else if (event.type === 'exit') {
          unsubscribe()
          resolve(parseGrepOutput(stdout))
        } else if (event.type === 'error') {
          unsubscribe()
          resolve([])
        }
      })
    }).catch(() => {
      resolve([])
    })
  })
}
