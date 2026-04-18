import { describe, it, expect, vi } from 'vitest'
import { searchDefinition } from './definitionSearch'
import type { ExecApi } from '../types'
import { ExecEventType, type ExecEvent } from '../../shared/ipc-types'

function createMockExecApi(opts: {
  startResult?: { success: boolean; execId?: string; error?: string }
  events?: ExecEvent[]
  startThrows?: boolean
}): ExecApi {
  const { startResult = { success: true, execId: 'exec-1' }, events = [], startThrows = false } = opts
  let eventCallback: ((event: ExecEvent) => void) | null = null

  return {
    start: vi.fn().mockImplementation(() => {
      if (startThrows) return Promise.reject(new Error('start failed'))
      if (!startResult.success) return Promise.resolve(startResult)
      // Fire events async after subscribe
      setTimeout(() => {
        for (const event of events) {
          eventCallback?.(event)
        }
      }, 0)
      return Promise.resolve(startResult)
    }),
    kill: vi.fn(),
    onEvent: vi.fn().mockImplementation((_execId: string, cb: (event: ExecEvent) => void) => {
      eventCallback = cb
      return vi.fn()
    }),
  }
}

describe('definitionSearch', () => {
  describe('searchDefinition covers all language switch branches', () => {
    it.each([
      ['typescript', '(function|const|let|var|class|interface|type|enum)\\s+mySymbol\\b'],
      ['javascript', '(function|const|let|var|class|interface|type|enum)\\s+mySymbol\\b'],
      ['python', '(def|class)\\s+mySymbol\\b'],
      ['rust', '(fn|struct|enum|trait|type|const|static|mod|impl)\\s+mySymbol\\b'],
      ['go', '(func|type|var|const)\\s+mySymbol\\b'],
      ['java', '(class|interface|enum|void|public|private|protected)\\s+.*mySymbol\\b'],
      ['kotlin', '(class|interface|enum|void|public|private|protected)\\s+.*mySymbol\\b'],
      ['ruby', '(def|class|module)\\s+mySymbol\\b'],
      ['php', '(function|class|interface|trait)\\s+mySymbol\\b'],
      ['unknown_lang', '(function|class|def|fn|type|interface|struct|enum)\\s+mySymbol\\b'],
    ] as const)('language %s produces correct pattern', async (language, expectedPattern) => {
      const execApi = createMockExecApi({
        events: [{ type: ExecEventType.Exit, exitCode: 0 }],
      })

      await searchDefinition(execApi, 'conn-1', '/workspace', 'mySymbol', language)

      expect(execApi.start).toHaveBeenCalledWith(
        'conn-1',
        '/workspace',
        'grep',
        expect.arrayContaining(['-rnE', expectedPattern])
      )
    })
  })

  describe('getFileGlobs via searchDefinition args', () => {
    it('typescript includes *.ts and *.tsx globs', async () => {
      const execApi = createMockExecApi({ events: [{ type: ExecEventType.Exit, exitCode: 0 }] })
      await searchDefinition(execApi, 'conn-1', '/ws', 'sym', 'typescript')
      const args = (execApi.start as ReturnType<typeof vi.fn>).mock.calls[0]![3] as string[]
      expect(args).toContain('--include=*.ts')
      expect(args).toContain('--include=*.tsx')
    })

    it('css includes css/scss/less globs', async () => {
      const execApi = createMockExecApi({ events: [{ type: ExecEventType.Exit, exitCode: 0 }] })
      await searchDefinition(execApi, 'conn-1', '/ws', 'sym', 'css')
      const args = (execApi.start as ReturnType<typeof vi.fn>).mock.calls[0]![3] as string[]
      expect(args).toContain('--include=*.css')
      expect(args).toContain('--include=*.scss')
      expect(args).toContain('--include=*.less')
    })

    it('html includes html/htm globs', async () => {
      const execApi = createMockExecApi({ events: [{ type: ExecEventType.Exit, exitCode: 0 }] })
      await searchDefinition(execApi, 'conn-1', '/ws', 'sym', 'html')
      const args = (execApi.start as ReturnType<typeof vi.fn>).mock.calls[0]![3] as string[]
      expect(args).toContain('--include=*.html')
      expect(args).toContain('--include=*.htm')
    })

    it('unknown language uses no include globs', async () => {
      const execApi = createMockExecApi({ events: [{ type: ExecEventType.Exit, exitCode: 0 }] })
      await searchDefinition(execApi, 'conn-1', '/ws', 'sym', 'brainfuck')
      const args = (execApi.start as ReturnType<typeof vi.fn>).mock.calls[0]![3] as string[]
      expect(args.filter((a: string) => a.startsWith('--include='))).toHaveLength(0)
    })

    it.each([
      ['javascript', ['*.js', '*.jsx', '*.mjs', '*.cjs']],
      ['python', ['*.py']],
      ['rust', ['*.rs']],
      ['go', ['*.go']],
      ['java', ['*.java']],
      ['kotlin', ['*.kt', '*.kts']],
      ['ruby', ['*.rb']],
      ['php', ['*.php']],
      ['scss', ['*.css', '*.scss', '*.less']],
      ['less', ['*.css', '*.scss', '*.less']],
    ] as const)('language %s includes correct file globs', async (language, expectedGlobs) => {
      const execApi = createMockExecApi({ events: [{ type: ExecEventType.Exit, exitCode: 0 }] })
      await searchDefinition(execApi, 'conn-1', '/ws', 'sym', language)
      const args = (execApi.start as ReturnType<typeof vi.fn>).mock.calls[0]![3] as string[]
      for (const glob of expectedGlobs) {
        expect(args).toContain(`--include=${glob}`)
      }
    })
  })

  describe('parseGrepOutput via searchDefinition result', () => {
    it('parses valid grep output lines', async () => {
      const execApi = createMockExecApi({
        events: [
          { type: ExecEventType.Stdout, data: 'src/foo.ts:10:function myFunc() {\nsrc/bar.ts:20:const myConst = 1\n' },
          { type: ExecEventType.Exit, exitCode: 0 },
        ],
      })
      const results = await searchDefinition(execApi, 'conn-1', '/ws', 'myFunc', 'typescript')
      expect(results).toEqual([
        { filePath: 'src/foo.ts', lineNumber: 10 },
        { filePath: 'src/bar.ts', lineNumber: 20 },
      ])
    })

    it('strips ./ prefix from file paths', async () => {
      const execApi = createMockExecApi({
        events: [
          { type: ExecEventType.Stdout, data: './src/foo.ts:5:class Foo {}\n' },
          { type: ExecEventType.Exit, exitCode: 0 },
        ],
      })
      const results = await searchDefinition(execApi, 'conn-1', '/ws', 'Foo', 'typescript')
      expect(results).toEqual([{ filePath: 'src/foo.ts', lineNumber: 5 }])
    })

    it('returns empty for no matches', async () => {
      const execApi = createMockExecApi({
        events: [
          { type: ExecEventType.Stdout, data: '' },
          { type: ExecEventType.Exit, exitCode: 1 },
        ],
      })
      const results = await searchDefinition(execApi, 'conn-1', '/ws', 'nope', 'typescript')
      expect(results).toEqual([])
    })
  })

  describe('error paths', () => {
    it('returns empty when exec.start fails', async () => {
      const execApi = createMockExecApi({ startResult: { success: false, error: 'no exec' } })
      const results = await searchDefinition(execApi, 'conn-1', '/ws', 'sym', 'typescript')
      expect(results).toEqual([])
    })

    it('returns empty when exec.start throws', async () => {
      const execApi = createMockExecApi({ startThrows: true })
      const results = await searchDefinition(execApi, 'conn-1', '/ws', 'sym', 'typescript')
      expect(results).toEqual([])
    })

    it('returns empty on error event', async () => {
      const execApi = createMockExecApi({
        events: [{ type: ExecEventType.Error, message: 'grep crashed' }],
      })
      const results = await searchDefinition(execApi, 'conn-1', '/ws', 'sym', 'typescript')
      expect(results).toEqual([])
    })
  })

  it('escapes special regex chars in symbol name', async () => {
    const execApi = createMockExecApi({ events: [{ type: ExecEventType.Exit, exitCode: 0 }] })
    await searchDefinition(execApi, 'conn-1', '/ws', 'foo.bar', 'typescript')
    const args = (execApi.start as ReturnType<typeof vi.fn>).mock.calls[0]![3] as string[]
    expect(args[1]).toContain('foo\\.bar')
  })
})
