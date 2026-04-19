/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Disallow calling .getState() in the render body of a React component or custom hook. ' +
        'Use useStore(store, s => s.field) to subscribe, or move the call into an event handler, useEffect, or useCallback.',
    },
    messages: {
      noGetStateInRender:
        '.getState() in a component/hook render body does not subscribe — the component will not re-render when the state changes. Use useStore(store, s => s.field), or move this call into an event handler / useEffect / useCallback.',
    },
    schema: [],
  },
  create(context) {
    function isComponentOrHookName(name) {
      if (!name) return false
      if (/^[A-Z]/.test(name)) return true
      if (/^use[A-Z]/.test(name)) return true
      return false
    }

    function getEnclosingFunctionName(fn) {
      if (fn.type === 'FunctionDeclaration' && fn.id) return fn.id.name
      const parent = fn.parent
      if (!parent) return null
      if (parent.type === 'VariableDeclarator' && parent.id && parent.id.type === 'Identifier') {
        return parent.id.name
      }
      if (parent.type === 'Property' && parent.key && parent.key.type === 'Identifier') {
        return parent.key.name
      }
      if (parent.type === 'AssignmentExpression' && parent.left.type === 'Identifier') {
        return parent.left.name
      }
      return null
    }

    return {
      'CallExpression[callee.type="MemberExpression"][callee.property.name="getState"][arguments.length=0]'(node) {
        let p = node.parent
        while (p) {
          if (
            p.type === 'FunctionDeclaration' ||
            p.type === 'FunctionExpression' ||
            p.type === 'ArrowFunctionExpression'
          ) {
            const name = getEnclosingFunctionName(p)
            if (isComponentOrHookName(name)) {
              context.report({ node, messageId: 'noGetStateInRender' })
            }
            return
          }
          p = p.parent
        }
      },
    }
  },
}
