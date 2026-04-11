import tseslint from 'typescript-eslint'
import reactPlugin from 'eslint-plugin-react'
import reactHooks from 'eslint-plugin-react-hooks'
import noStringLiteralUnion from './eslint-rules/no-string-literal-union.js'

export default tseslint.config(
  {
    ignores: ['src/__mocks__/**', 'src/generated/**']
  },
  ...tseslint.configs.strictTypeChecked.map(config => ({
    ...config,
    files: ['src/**/*.ts', 'src/**/*.tsx'],
  })),
  {
    files: ['src/**/*.ts', 'src/**/*.tsx'],
    languageOptions: {
      parserOptions: {
        project: ['./tsconfig.node.json', './tsconfig.web.json']
      }
    },
    plugins: {
      custom: { rules: { 'no-string-literal-union': noStringLiteralUnion } }
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
      'custom/no-string-literal-union': 'error',
      'no-restricted-syntax': ['error', {
        selector: "TSTypeReference[typeName.name='Pick']",
        message: 'Pick<T, K> is banned. Pass the full type instead of narrowing with Pick.'
      }]
    }
  },
  {
    files: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-non-null-assertion': 'off',
    }
  },
  {
    files: ['src/main/**/*.ts'],
    ignores: ['src/main/ipc/ipc-server.ts', 'src/main/ipc/ipc-server.test.ts'],
    rules: {
      'no-restricted-imports': ['error', {
        paths: [{
          name: 'electron',
          importNames: ['ipcMain'],
          message: 'ipcMain is only allowed in ipc-server.ts. Use IpcServer methods instead.'
        }]
      }]
    }
  },
  {
    files: ['src/preload/**/*.ts'],
    ignores: ['src/preload/ipc-client.ts', 'src/preload/ipc-client.test.ts'],
    rules: {
      'no-restricted-imports': ['error', {
        paths: [{
          name: 'electron',
          importNames: ['ipcRenderer'],
          message: 'ipcRenderer is only allowed in ipc-client.ts. Use IpcClient methods instead.'
        }]
      }]
    }
  },
  {
    files: ['src/renderer/**/*.ts', 'src/renderer/**/*.tsx'],
    ignores: ['src/renderer/main.tsx'],
    plugins: {
      'react': reactPlugin,
      'react-hooks': reactHooks
    },
    rules: {
      'react/no-unstable-nested-components': 'error',
      'react/jsx-no-constructed-context-values': 'error',
      'react/no-array-index-key': 'warn',
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      'react-hooks/set-state-in-effect': 'error',
      'react-hooks/set-state-in-render': 'error',
      'no-restricted-syntax': ['error', {
        selector: "MemberExpression[object.name='window']",
        message: 'Direct window access is only allowed in main.tsx. Inject dependencies via useAppStore() or function parameters instead.'
      }, {
        selector: "CallExpression[callee.name=/^use.*Store$/] > ArrowFunctionExpression CallExpression[callee.property.name=/^(filter|map|flatMap|slice|concat|flat|reduce)$/]",
        message: 'Zustand selectors must return stable references. Array methods like .filter()/.map() create new arrays every call, causing infinite re-renders. Select the parent object and derive in the function body.'
      }, {
        selector: "CallExpression[callee.name=/^use.*Store$/] > ArrowFunctionExpression CallExpression[callee.object.name='Object'][callee.property.name=/^(values|keys|entries|assign)$/]",
        message: 'Zustand selectors must return stable references. Object.values()/keys()/entries() create new arrays every call. Select the parent object and derive in the function body.'
      }, {
        selector: "CallExpression[callee.name='useStore']:not([arguments.1])",
        message: 'useStore() must have a selector: useStore(store, s => s.field). Subscribing to the entire store causes re-renders on every state change.'
      }]
    }
  }
)
