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
    const resolvedDir = path.resolve(workspacePath, dirPath)

    // Security check
    if (!isPathWithinWorkspace(workspacePath, resolvedDir)) {
      return { success: false, error: 'Access denied: Path outside workspace' }
    }

    const entries = await fs.readdir(resolvedDir, { withFileTypes: true })
    const fileEntries = await Promise.all(
      entries
        .filter((entry) => !entry.name.startsWith('.')) // Hide hidden files by default
        .map(async (entry) => {
          const fullPath = path.join(resolvedDir, entry.name)
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
      contents: { path: resolvedDir, entries: fileEntries }
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
    const resolvedFile = path.resolve(workspacePath, filePath)

    // Security check
    if (!isPathWithinWorkspace(workspacePath, resolvedFile)) {
      return { success: false, error: 'Access denied: Path outside workspace' }
    }

    const stats = await fs.stat(resolvedFile)

    // Limit file size (1MB)
    const MAX_SIZE = 1024 * 1024
    if (stats.size > MAX_SIZE) {
      return { success: false, error: 'File too large to preview (max 1MB)' }
    }

    const content = await fs.readFile(resolvedFile, 'utf-8')
    const language = detectLanguage(resolvedFile)

    return {
      success: true,
      file: {
        path: resolvedFile,
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
    const resolvedFile = path.resolve(workspacePath, filePath)

    // Security check
    if (!isPathWithinWorkspace(workspacePath, resolvedFile)) {
      return { success: false, error: 'Access denied: Path outside workspace' }
    }

    // Ensure parent directory exists
    await fs.mkdir(path.dirname(resolvedFile), { recursive: true })

    // Write file
    await fs.writeFile(resolvedFile, content, 'utf-8')

    return { success: true }
  } catch (error) {
    return { success: false, error: String(error) }
  }
}

export async function searchFiles(
  workspacePath: string,
  query: string
): Promise<{ success: boolean; entries?: FileEntry[]; error?: string }> {
  try {
    // Security check
    if (!isPathWithinWorkspace(workspacePath, workspacePath)) {
      return { success: false, error: 'Access denied: Invalid workspace path' }
    }

    const normalizedQuery = query.toLowerCase().trim()
    if (!normalizedQuery) {
      return { success: true, entries: [] }
    }

    const results: FileEntry[] = []

    const walkDir = async (dirPath: string): Promise<void> => {
      try {
        const entries = await fs.readdir(dirPath, { withFileTypes: true })

        for (const entry of entries) {
          // Skip hidden files and node_modules
          if (entry.name.startsWith('.') || entry.name === 'node_modules') {
            continue
          }

          const fullPath = path.join(dirPath, entry.name)
          const relativePath = path.relative(workspacePath, fullPath)

          // Check if name matches query
          const nameLower = entry.name.toLowerCase()
          const matches = nameLower.includes(normalizedQuery)

          let stats = null
          try {
            stats = await fs.stat(fullPath)
          } catch {
            // Ignore stat errors
          }

          const fileEntry: FileEntry = {
            name: entry.name,
            path: fullPath,
            relativePath,
            isDirectory: entry.isDirectory(),
            size: stats?.size,
            modifiedTime: stats?.mtimeMs
          }

          if (matches) {
            results.push(fileEntry)
          }

          // Recurse into directories
          if (entry.isDirectory()) {
            await walkDir(fullPath)
          }
        }
      } catch (error) {
        // Ignore errors for individual directories (e.g., permission denied)
      }
    }

    await walkDir(workspacePath)

    // Sort: directories first, then alphabetically
    results.sort((a, b) => {
      if (a.isDirectory !== b.isDirectory) {
        return a.isDirectory ? -1 : 1
      }
      return a.name.localeCompare(b.name)
    })

    return { success: true, entries: results }
  } catch (error) {
    return { success: false, error: String(error) }
  }
}
