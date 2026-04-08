/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Disallow string literal union types in favor of enums',
    },
    messages: {
      useEnum: 'Use an enum instead of a string literal union type.',
    },
    schema: [],
  },
  create(context) {
    return {
      TSTypeAliasDeclaration(node) {
        const { typeAnnotation } = node
        if (typeAnnotation.type !== 'TSUnionType') return
        const { types } = typeAnnotation
        if (types.length < 2) return

        const allStringLiterals = types.every(
          (t) =>
            t.type === 'TSLiteralType' &&
            t.literal.type === 'Literal' &&
            typeof t.literal.value === 'string',
        )

        if (allStringLiterals) {
          context.report({ node: node.id, messageId: 'useEnum' })
        }
      },
    }
  },
}
