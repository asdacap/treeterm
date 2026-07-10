import { RuleTester } from 'eslint'
import tsParser from '@typescript-eslint/parser'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import rule from './no-discarded-disposable.js'

const fixtures = join(dirname(fileURLToPath(import.meta.url)), 'fixtures')

const tester = new RuleTester({
  languageOptions: {
    parser: tsParser,
    parserOptions: {
      project: './tsconfig.json',
      tsconfigRootDir: fixtures,
    },
  },
})

// Prepended to every case so the checker knows what a disposable looks like.
const PRELUDE = `
interface IDisposable { dispose(): void }
declare class DisposableStore implements IDisposable {
  add<T extends IDisposable>(d: T): T
  addFn(fn: () => void): IDisposable
  dispose(): void
}
declare function thenRegisterOrDispose<T extends IDisposable>(p: Promise<T>, o: DisposableStore): Promise<T>
declare function openTtyStream(): Promise<IDisposable>
declare function makeSubscription(): IDisposable
declare function plainCall(): number
declare const owner: DisposableStore
declare const sub: IDisposable
`

const wrap = (code) => ({ code: PRELUDE + code, filename: join(fixtures, 'file.ts') })
const invalid = (code) => ({ ...wrap(code), errors: [{ messageId: 'discarded' }] })

tester.run('no-discarded-disposable', rule, {
  valid: [
    // Captured in a binding — someone can still dispose it.
    wrap('const s = makeSubscription()'),
    // Handed to an owner.
    wrap('owner.add(makeSubscription())'),
    wrap('owner.addFn(() => {})'),
    wrap('thenRegisterOrDispose(openTtyStream(), owner)'),
    wrap('void thenRegisterOrDispose(openTtyStream(), owner)'),
    // Disposing is not discarding.
    wrap('sub.dispose()'),
    // Nothing owns a resource here.
    wrap('plainCall()'),
    // Assignment keeps the value.
    wrap('let x: IDisposable; x = makeSubscription()'),
  ],
  invalid: [
    // The bug this rule exists for: the subscription is dropped on the floor.
    invalid('makeSubscription()'),
    // Floating promise of a disposable — the Tty lands and nobody owns it.
    invalid('openTtyStream()'),
    invalid('await openTtyStream()'),
    // `void` is not an escape hatch; it leaks exactly the same.
    invalid('void makeSubscription()'),
    // A non-sink method returning a disposable is still a discard.
    invalid('owner.add(makeSubscription()).dispose; makeSubscription()'),
  ],
})

console.log('no-discarded-disposable: all tests passed')
