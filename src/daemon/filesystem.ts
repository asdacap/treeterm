import * as fs from 'fs/promises'
import * as path from 'path'

// Security: Ensure path is within workspace
function isPathWithinWorkspace(workspacePath: string, targetPath: string): boolean {
  const resolvedWorkspace = path.resolve(workspacePath)
  const resolvedTarget = path.resolve(targetPath)
  return (
    resolvedTarget.startsWith(resolvedWorkspace + path.sep) || resolvedTarget === resolvedWorkspace
  )
}

// Detect language from file extension for syntax highlighting
function detectLanguage(filePath: string): string {
  const ext = path.extname(filePath).toLowerCase()
  const languageMap: Record<string, string> = {
    '.ts': 'typescript',
    '.tsx': 'typescript',
    '.js': 'javascript',
    '.jsx': 'javascript',
    '.json': 'json',
    '.md': 'markdown',
    '.css': 'css',
    '.scss': 'scss',
    '.less': 'less',
    '.html': 'html',
    '.htm': 'html',
    '.xml': 'xml',
    '.py': 'python',
    '.rs': 'rust',
    '.go': 'go',
    '.java': 'java',
    '.c': 'c',
    '.cpp': 'cpp',
    '.h': 'c',
    '.hpp': 'cpp',
    '.yaml': 'yaml',
    '.yml': 'yaml',
    '.toml': 'toml',
    '.sh': 'bash',
    '.bash': 'bash',
    '.zsh': 'bash',
    '.sql': 'sql',
    '.graphql': 'graphql',
    '.gql': 'graphql',
    '.vue': 'html',
    '.svelte': 'html',
    '.rb': 'ruby',
    '.php': 'php',
    '.swift': 'swift',
    '.kt': 'kotlin',
    '.kts': 'kotlin',
    '.scala': 'scala',
    '.r': 'r',
    '.R': 'r',
    '.lua': 'lua',
    '.dockerfile': 'dockerfile',
    '.gitignore': 'plaintext',
    '.env': 'plaintext'
  }
  return languageMap[ext] || 'plaintext'
}

export interface FileEntry {
  name: string
  path: string
  relativePath: string
  isDirectory: boolean
  size?: number
  modifiedTime?: number
}

export interface DirectoryContents {
  path: string
  entries: FileEntry[]
}

export interface FileContents {
  path: string
  content: string
  size: number
  language: string
}

export async function readDirectory(
  workspacePath: string,
  dirPath: string
): Promise<{ success: boolean; contents?: DirectoryContents; error?: string }> {
  try {
    // Security check
    if (!isPathWithinWorkspace(workspacePath, dirPath)) {
      return { success: false, error: 'Access denied: Path outside workspace' }
    }

    const entries = await fs.readdir(dirPath, { withFileTypes: true })
    const fileEntries = await Promise.all(
      entries
        .filter((entry) => !entry.name.startsWith('.')) // Hide hidden files by default
        .map(async (entry) => {
          const fullPath = path.join(dirPath, entry.name)
          const relativePath = path.relative(workspacePath, fullPath)
          let stats = null
          try {
            stats = await fs.stat(fullPath)
          } catch {
            // Ignore stat errors
          }

          return {
            name: entry.name,
            path: fullPath,
            relativePath,
            isDirectory: entry.isDirectory(),
            size: stats?.size,
            modifiedTime: stats?.mtimeMs
          }
        })
    )

    // Sort: directories first, then alphabetically
    fileEntries.sort((a, b) => {
      if (a.isDirectory !== b.isDirectory) {
        return a.isDirectory ? -1 : 1
      }
      return a.name.localeCompare(b.name)
    })

    return {
      success: true,
      contents: { path: dirPath, entries: fileEntries }
    }
  } catch (error) {
    return { success: false, error: String(error) }
  }
}

export async function readFile(
  workspacePath: string,
  filePath: string
): Promise<{ success: boolean; file?: FileContents; error?: string }> {
  try {
    // Security check
    if (!isPathWithinWorkspace(workspacePath, filePath)) {
      return { success: false, error: 'Access denied: Path outside workspace' }
    }

    const stats = await fs.stat(filePath)

    // Limit file size (1MB)
    const MAX_SIZE = 1024 * 1024
    if (stats.size > MAX_SIZE) {
      return { success: false, error: 'File too large to preview (max 1MB)' }
    }

    const content = await fs.readFile(filePath, 'utf-8')
    const language = detectLanguage(filePath)

    return {
      success: true,
      file: {
        path: filePath,
        content,
        size: stats.size,
        language
      }
    }
  } catch (error) {
    return { success: false, error: String(error) }
  }
}

export async function writeFile(
  workspacePath: string,
  filePath: string,
  content: string
): Promise<{ success: boolean; error?: string }> {
  try {
    // Security check
    if (!isPathWithinWorkspace(workspacePath, filePath)) {
      return { success: false, error: 'Access denied: Path outside workspace' }
    }

    // Write file
    await fs.writeFile(filePath, content, 'utf-8')

    return { success: true }
  } catch (error) {
    return { success: false, error: String(error) }
  }
}
