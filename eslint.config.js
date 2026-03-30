import tseslint from '@typescript-eslint/eslint-plugin'
import tsparser from '@typescript-eslint/parser'
import reactHooks from 'eslint-plugin-react-hooks'

export default [
  {
    ignores: ['src/__mocks__/**', 'src/generated/**']
  },
  {
    files: ['src/**/*.ts', 'src/**/*.tsx'],
    languageOptions: {
      parser: tsparser,
      parserOptions: {
        project: ['./tsconfig.node.json', './tsconfig.web.json']
      }
    },
    plugins: {
      '@typescript-eslint': tseslint
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error'
    }
  },
  {
    files: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off'
    }
  },
  {
    files: ['src/renderer/**/*.ts', 'src/renderer/**/*.tsx'],
    ignores: ['src/renderer/main.tsx'],
    plugins: {
      'react-hooks': reactHooks
    },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'no-restricted-syntax': ['error', {
        selector: "MemberExpression[object.name='window'][property.name='electron']",
        message: 'window.electron is only allowed in main.tsx. Use useAppStore() instead.'
      }, {
        selector: "CallExpression[callee.name=/^use.*Store$/] > ArrowFunctionExpression CallExpression[callee.property.name=/^(filter|map|flatMap|slice|concat|flat|reduce)$/]",
        message: 'Zustand selectors must return stable references. Array methods like .filter()/.map() create new arrays every call, causing infinite re-renders. Select the parent object and derive with useMemo().'
      }, {
        selector: "CallExpression[callee.name=/^use.*Store$/] > ArrowFunctionExpression CallExpression[callee.object.name='Object'][callee.property.name=/^(values|keys|entries|assign)$/]",
        message: 'Zustand selectors must return stable references. Object.values()/keys()/entries() create new arrays every call. Select the parent object and derive with useMemo().'
      }]
    }
  }
]
