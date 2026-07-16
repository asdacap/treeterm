import { describe, expect, it } from 'vitest'
import { detectMonacoLanguage } from './detectMonacoLanguage'

describe('detectMonacoLanguage', () => {
  it.each([
    ['src/example.ts', 'typescript'],
    ['src/example.tsx', 'typescript'],
    ['src/example.js', 'javascript'],
    ['src/example.jsx', 'javascript'],
    ['src/example.json', 'json'],
    ['src/example.md', 'markdown'],
    ['src/example.css', 'css'],
    ['src/example.scss', 'scss'],
    ['src/example.less', 'less'],
    ['src/example.html', 'html'],
    ['src/example.htm', 'html'],
    ['src/example.vue', 'html'],
    ['src/example.svelte', 'html'],
    ['src/example.xml', 'xml'],
    ['src/example.py', 'python'],
    ['src/example.rs', 'rust'],
    ['src/example.go', 'go'],
    ['src/example.java', 'java'],
    ['src/example.cs', 'csharp'],
    ['src/example.c', 'c'],
    ['src/example.h', 'c'],
    ['src/example.cpp', 'cpp'],
    ['src/example.hpp', 'cpp'],
    ['src/example.yaml', 'yaml'],
    ['src/example.yml', 'yaml'],
    ['src/example.toml', 'toml'],
    ['src/example.sh', 'shell'],
    ['src/example.bash', 'shell'],
    ['src/example.zsh', 'shell'],
    ['src/example.sql', 'sql'],
    ['src/example.graphql', 'graphql'],
    ['src/example.gql', 'graphql'],
    ['src/example.rb', 'ruby'],
    ['src/example.php', 'php'],
    ['src/example.swift', 'swift'],
    ['src/example.kt', 'kotlin'],
    ['src/example.kts', 'kotlin'],
    ['src/example.scala', 'scala'],
    ['src/example.r', 'r'],
    ['src/example.lua', 'lua'],
    ['src/example.dockerfile', 'dockerfile'],
  ])('detects %s as %s', (filePath, expectedLanguage) => {
    expect(detectMonacoLanguage(filePath)).toBe(expectedLanguage)
  })

  it('detects C# case-insensitively from a Windows path', () => {
    expect(detectMonacoLanguage('C:\\repo\\Example.CS')).toBe('csharp')
  })

  it.each(['README', 'src/example.unknown'])('falls back to plaintext for %s', (filePath) => {
    expect(detectMonacoLanguage(filePath)).toBe('plaintext')
  })
})
