import { RuleTester } from 'eslint'
import rule from './no-string-literal-comparison.js'

const tester = new RuleTester({
  languageOptions: { ecmaVersion: 2022, sourceType: 'module' },
})

tester.run('no-string-literal-comparison', rule, {
  valid: [
    { code: 'if (x === Status.Active) {}' },
    { code: 'if (typeof x === "string") {}' },
    { code: 'if (typeof x !== "undefined") {}' },
    { code: 'if ("string" === typeof x) {}' },
    { code: 'switch (x) { case Status.Active: break; default: break; }' },
    { code: 'if (x === 42) {}' },
    { code: 'if (x === true) {}' },
    { code: 'if (x === null) {}' },
    { code: 'const p = prefix; if (x === `${p}foo`) {}' },
  ],
  invalid: [
    {
      code: 'if (x === "foo") {}',
      errors: [{ messageId: 'useEnum' }],
    },
    {
      code: 'if (x !== "foo") {}',
      errors: [{ messageId: 'useEnum' }],
    },
    {
      code: 'if (x == "foo") {}',
      errors: [{ messageId: 'useEnum' }],
    },
    {
      code: 'if (x != "foo") {}',
      errors: [{ messageId: 'useEnum' }],
    },
    {
      code: 'if ("foo" === x) {}',
      errors: [{ messageId: 'useEnum' }],
    },
    {
      code: 'if (x === `foo`) {}',
      errors: [{ messageId: 'useEnum' }],
    },
    {
      code: 'switch (x) { case "a": break; case "b": break; default: break; }',
      errors: [{ messageId: 'useEnum' }, { messageId: 'useEnum' }],
    },
    {
      code: 'if (x === "foo" && y === "bar") {}',
      errors: [{ messageId: 'useEnum' }, { messageId: 'useEnum' }],
    },
    {
      code: 'if ("a" === "b") {}',
      errors: [{ messageId: 'useEnum' }, { messageId: 'useEnum' }],
    },
  ],
})

console.log('no-string-literal-comparison: all tests passed')
