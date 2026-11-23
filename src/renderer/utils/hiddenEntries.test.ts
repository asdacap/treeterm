import { describe, expect, it } from 'vitest'
import type { FileEntry } from '../types'
import { withoutHiddenEntries } from './hiddenEntries'

const entry = (name: string): FileEntry => ({ name, path: `/repo/${name}`, relativePath: name, isDirectory: false })

describe('withoutHiddenEntries', () => {
  it('drops only the entries whose name starts with a dot', () => {
    const names = withoutHiddenEntries([entry('.env'), entry('.git'), entry('a.txt'), entry('dir.d')]).map((e) => e.name)
    expect(names).toEqual(['a.txt', 'dir.d'])
  })
})
