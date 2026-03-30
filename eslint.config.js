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
      }]
    }
  }
]
