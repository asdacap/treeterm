/**
 * Disallow throwing away a value that owns a resource.
 *
 * This is the rule that a bare `() => void` cleanup could never support: an unsubscribe
 * function is shaped exactly like every other callback, so nothing could tell the two
 * apart. Once cleanup is carried by a named `IDisposable`, the type checker can.
 *
 * A disposable reaching an expression statement means nobody kept it, so nobody will
 * ever dispose it — unless the call itself transfers ownership (`store.add(x)`), which
 * is why OWNERSHIP_SINKS exists.
 */

/** Calls that hand the disposable to an owner, so their return value is safe to drop. */
const OWNERSHIP_SINKS = new Set([
  'add',
  'addFn',
  'set',
  'deleteAndDispose',
  'thenRegisterOrDispose',
])

function calleeName(node) {
  if (node.type !== 'CallExpression') return null
  const callee = node.callee
  if (callee.type === 'Identifier') return callee.name
  if (callee.type === 'MemberExpression' && callee.property.type === 'Identifier') {
    return callee.property.name
  }
  return null
}

/** True when `type` has a callable `dispose` member. */
function hasDisposeMethod(checker, type, tsNode) {
  if (type.isUnion() || type.isIntersection()) {
    return type.types.some((member) => hasDisposeMethod(checker, member, tsNode))
  }
  const symbol = checker.getPropertyOfType(type, 'dispose')
  if (!symbol) return false
  const disposeType = checker.getTypeOfSymbolAtLocation(symbol, tsNode)
  return disposeType.getCallSignatures().length > 0
}

/** Unwraps `Promise<T>` once, so a floating `openTtyStream(...)` is caught too. */
function unwrapPromise(checker, type) {
  if (type.symbol?.name !== 'Promise') return type
  const args = checker.getTypeArguments(type)
  return args.length === 1 ? args[0] : type
}

/**
 * Third-party listener handles (monaco's `editor.onDidScrollChange`, xterm's
 * `addon.onContextLoss`) are owned by the emitter, which disposes them with itself. We
 * cannot manage those lifetimes and should not pretend to. This rule governs the
 * disposables this codebase declares.
 */
function isDeclaredInNodeModules(type) {
  const declarations = type.getSymbol()?.getDeclarations()
  if (!declarations?.length) return false
  return declarations.every((declaration) =>
    declaration.getSourceFile().fileName.includes('/node_modules/'),
  )
}

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Disallow discarding an IDisposable. Give it to a DisposableStore, or dispose it explicitly.',
    },
    messages: {
      discarded:
        'This value owns a resource ({{what}}) and is being discarded, so nothing will ever dispose it. ' +
        'Hand it to a DisposableStore (`owner.add(...)` / `thenRegisterOrDispose(...)`), or call .dispose() on it.',
    },
    schema: [],
  },
  create(context) {
    const services = context.sourceCode.parserServices
    // Without type information this rule cannot tell a disposable from any other object.
    if (!services?.program || !services.esTreeNodeToTSNodeMap) return {}
    const checker = services.program.getTypeChecker()

    return {
      ExpressionStatement(node) {
        let expression = node.expression
        // `void foo()` is not an escape hatch — a discarded disposable leaks either way.
        if (expression.type === 'UnaryExpression' && expression.operator === 'void') {
          expression = expression.argument
        }
        if (expression.type === 'AwaitExpression') expression = expression.argument
        // `x = foo()` keeps the value; the assignment target is someone else's problem.
        if (expression.type === 'AssignmentExpression') return

        const sink = calleeName(expression)
        if (sink && OWNERSHIP_SINKS.has(sink)) return

        const tsNode = services.esTreeNodeToTSNodeMap.get(expression)
        if (!tsNode) return

        const type = unwrapPromise(checker, checker.getTypeAtLocation(tsNode))
        if (!hasDisposeMethod(checker, type, tsNode)) return
        if (isDeclaredInNodeModules(type)) return

        context.report({
          node: expression,
          messageId: 'discarded',
          data: { what: checker.typeToString(type) },
        })
      },
    }
  },
}
