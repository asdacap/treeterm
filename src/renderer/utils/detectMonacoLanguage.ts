const LANGUAGE_BY_EXTENSION: Readonly<Record<string, string>> = {
  ts: 'typescript',
  tsx: 'typescript',
  js: 'javascript',
  jsx: 'javascript',
  json: 'json',
  md: 'markdown',
  css: 'css',
  scss: 'scss',
  less: 'less',
  html: 'html',
  htm: 'html',
  vue: 'html',
  svelte: 'html',
  xml: 'xml',
  py: 'python',
  rs: 'rust',
  go: 'go',
  java: 'java',
  cs: 'csharp',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  hpp: 'cpp',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  sh: 'shell',
  bash: 'shell',
  zsh: 'shell',
  sql: 'sql',
  graphql: 'graphql',
  gql: 'graphql',
  rb: 'ruby',
  php: 'php',
  swift: 'swift',
  kt: 'kotlin',
  kts: 'kotlin',
  scala: 'scala',
  r: 'r',
  lua: 'lua',
  dockerfile: 'dockerfile',
}

export function detectMonacoLanguage(filePath: string): string {
  const fileName = filePath.split(/[\\/]/).pop() ?? filePath
  const extensionSeparatorIndex = fileName.lastIndexOf('.')
  if (extensionSeparatorIndex < 0) return 'plaintext'

  const extension = fileName.slice(extensionSeparatorIndex + 1).toLowerCase()
  return LANGUAGE_BY_EXTENSION[extension] ?? 'plaintext'
}
