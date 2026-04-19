import { RuleTester } from 'eslint'
import rule from './no-get-state-in-render.js'

const tester = new RuleTester({
  languageOptions: { ecmaVersion: 2022, sourceType: 'module' },
})

tester.run('no-get-state-in-render', rule, {
  valid: [
    // Non-component, non-hook function — allowed
    { code: 'function buildModel(ws) { return ws.getState().workspace }' },
    { code: 'function helper() { store.getState() }' },
    { code: 'const helper = () => store.getState()' },

    // Inside a nested function (handler) within a component — allowed
    { code: 'function Foo() { const onClick = () => store.getState().doIt() }' },
    { code: 'function Foo() { const onClick = (e) => { store.getState().doIt(e) } }' },

    // Inside useEffect / useCallback / useMemo — allowed (nested arrow)
    { code: 'function Foo() { useEffect(() => { store.getState() }, []) }' },
    { code: 'function Foo() { const x = useCallback(() => store.getState(), []) }' },

    // Inside a hook's nested callback — allowed
    { code: 'function useFoo() { useEffect(() => { store.getState() }, []) }' },

    // Module-level — allowed (no enclosing function)
    { code: 'const initial = store.getState()' },

    // getState with arguments is not the zustand API — ignore
    { code: 'function Foo() { store.getState(arg) }' },

    // Non-component arrow assigned to lowercase — allowed
    { code: 'const helper = () => { store.getState() }' },
  ],
  invalid: [
    // Direct call in component render body
    {
      code: 'function Foo() { const x = store.getState() }',
      errors: [{ messageId: 'noGetStateInRender' }],
    },
    // Arrow component
    {
      code: 'const Foo = () => { const x = store.getState(); return null }',
      errors: [{ messageId: 'noGetStateInRender' }],
    },
    // Custom hook render body
    {
      code: 'function useFoo() { const x = store.getState() }',
      errors: [{ messageId: 'noGetStateInRender' }],
    },
    // Chained getState (nested store)
    {
      code: 'function Foo() { const x = entry.store.getState().gitController.getState() }',
      errors: [{ messageId: 'noGetStateInRender' }, { messageId: 'noGetStateInRender' }],
    },
    // Ternary in render body
    {
      code: 'function Foo() { const x = cond ? entry.store.getState() : undefined }',
      errors: [{ messageId: 'noGetStateInRender' }],
    },
    // export default
    {
      code: 'export default function Foo() { store.getState() }',
      errors: [{ messageId: 'noGetStateInRender' }],
    },
  ],
})

console.log('no-get-state-in-render: all tests passed')
