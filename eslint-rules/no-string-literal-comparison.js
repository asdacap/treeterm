/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Disallow comparing values to string constants; use an enum instead.',
    },
    messages: {
      useEnum: 'Compare against an enum value instead of a string literal.',
    },
    schema: [],
  },
  create(context) {
    const isStringConst = (n) =>
      (n.type === 'Literal' && typeof n.value === 'string') ||
      (n.type === 'TemplateLiteral' && n.expressions.length === 0)
    const isTypeof = (n) => n.type === 'UnaryExpression' && n.operator === 'typeof'
    const eqOps = new Set(['===', '!==', '==', '!='])
    return {
      BinaryExpression(node) {
        if (!eqOps.has(node.operator)) return
        if (isStringConst(node.left) && !isTypeof(node.right)) {
          context.report({ node: node.left, messageId: 'useEnum' })
        }
        if (isStringConst(node.right) && !isTypeof(node.left)) {
          context.report({ node: node.right, messageId: 'useEnum' })
        }
      },
      SwitchCase(node) {
        if (node.test && isStringConst(node.test)) {
          context.report({ node: node.test, messageId: 'useEnum' })
        }
      },
    }
  },
}
