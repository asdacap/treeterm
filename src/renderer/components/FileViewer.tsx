import { useState, useEffect, useMemo } from 'react'
import hljs from 'highlight.js/lib/core'
import typescript from 'highlight.js/lib/languages/typescript'
import javascript from 'highlight.js/lib/languages/javascript'
import json from 'highlight.js/lib/languages/json'
import markdown from 'highlight.js/lib/languages/markdown'
import css from 'highlight.js/lib/languages/css'
import xml from 'highlight.js/lib/languages/xml'
import python from 'highlight.js/lib/languages/python'
import rust from 'highlight.js/lib/languages/rust'
import go from 'highlight.js/lib/languages/go'
import java from 'highlight.js/lib/languages/java'
import c from 'highlight.js/lib/languages/c'
import cpp from 'highlight.js/lib/languages/cpp'
import yaml from 'highlight.js/lib/languages/yaml'
import bash from 'highlight.js/lib/languages/bash'
import sql from 'highlight.js/lib/languages/sql'
import ruby from 'highlight.js/lib/languages/ruby'
import php from 'highlight.js/lib/languages/php'
import swift from 'highlight.js/lib/languages/swift'
import kotlin from 'highlight.js/lib/languages/kotlin'
import scala from 'highlight.js/lib/languages/scala'
import lua from 'highlight.js/lib/languages/lua'
import dockerfile from 'highlight.js/lib/languages/dockerfile'
import graphql from 'highlight.js/lib/languages/graphql'
import scss from 'highlight.js/lib/languages/scss'
import less from 'highlight.js/lib/languages/less'
import plaintext from 'highlight.js/lib/languages/plaintext'

// Register languages
hljs.registerLanguage('typescript', typescript)
hljs.registerLanguage('javascript', javascript)
hljs.registerLanguage('json', json)
hljs.registerLanguage('markdown', markdown)
hljs.registerLanguage('css', css)
hljs.registerLanguage('html', xml)
hljs.registerLanguage('xml', xml)
hljs.registerLanguage('python', python)
hljs.registerLanguage('rust', rust)
hljs.registerLanguage('go', go)
hljs.registerLanguage('java', java)
hljs.registerLanguage('c', c)
hljs.registerLanguage('cpp', cpp)
hljs.registerLanguage('yaml', yaml)
hljs.registerLanguage('bash', bash)
hljs.registerLanguage('sql', sql)
hljs.registerLanguage('ruby', ruby)
hljs.registerLanguage('php', php)
hljs.registerLanguage('swift', swift)
hljs.registerLanguage('kotlin', kotlin)
hljs.registerLanguage('scala', scala)
hljs.registerLanguage('lua', lua)
hljs.registerLanguage('dockerfile', dockerfile)
hljs.registerLanguage('graphql', graphql)
hljs.registerLanguage('scss', scss)
hljs.registerLanguage('less', less)
hljs.registerLanguage('plaintext', plaintext)

interface FileViewerProps {
  workspacePath: string
  filePath: string | null
}

interface FileState {
  content: string
  language: string
  loading: boolean
  error: string | null
}

export function FileViewer({ workspacePath, filePath }: FileViewerProps): JSX.Element {
  const [fileState, setFileState] = useState<FileState>({
    content: '',
    language: 'plaintext',
    loading: false,
    error: null
  })

  useEffect(() => {
    if (!filePath) {
      setFileState({ content: '', language: 'plaintext', loading: false, error: null })
      return
    }

    const loadFile = async () => {
      setFileState((prev) => ({ ...prev, loading: true, error: null }))

      const result = await window.electron.filesystem.readFile(workspacePath, filePath)

      if (result.success && result.file) {
        setFileState({
          content: result.file.content,
          language: result.file.language,
          loading: false,
          error: null
        })
      } else {
        setFileState({
          content: '',
          language: 'plaintext',
          loading: false,
          error: result.error || 'Failed to load file'
        })
      }
    }

    loadFile()
  }, [workspacePath, filePath])

  const highlightedContent = useMemo(() => {
    if (!fileState.content) return []

    try {
      const highlighted = hljs.highlight(fileState.content, {
        language: fileState.language,
        ignoreIllegals: true
      })
      return highlighted.value.split('\n')
    } catch {
      // Fallback to plaintext if highlighting fails
      return fileState.content.split('\n')
    }
  }, [fileState.content, fileState.language])

  if (!filePath) {
    return (
      <div className="file-viewer">
        <div className="file-viewer-placeholder">Select a file to view its contents</div>
      </div>
    )
  }

  if (fileState.loading) {
    return (
      <div className="file-viewer">
        <div className="file-viewer-placeholder">Loading...</div>
      </div>
    )
  }

  if (fileState.error) {
    return (
      <div className="file-viewer">
        <div className="file-viewer-error">{fileState.error}</div>
      </div>
    )
  }

  const fileName = filePath.split('/').pop() || filePath

  return (
    <div className="file-viewer">
      <div className="file-viewer-header">
        <span className="file-viewer-filename">{fileName}</span>
        <span className="file-viewer-language">{fileState.language}</span>
      </div>
      <div className="file-viewer-content">
        <table className="file-viewer-table">
          <tbody>
            {highlightedContent.map((line, index) => (
              <tr key={index} className="file-viewer-line">
                <td className="file-viewer-line-number">{index + 1}</td>
                <td
                  className="file-viewer-line-content"
                  dangerouslySetInnerHTML={{ __html: line || '&nbsp;' }}
                />
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
