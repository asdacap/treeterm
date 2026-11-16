; From tree-sitter-python 0.23.6 (MIT), its `HIGHLIGHTS_QUERY`. TreeTerm: control-flow and
; import keywords are `@keyword.control`, as VS Code's Python grammar scopes them
; (`keyword.control.flow.python`, `keyword.control.import.python`) and Dark+ colours them
; apart from other keywords.

; Identifier naming conventions

(identifier) @variable

((identifier) @constructor
 (#match? @constructor "^[A-Z]"))

((identifier) @constant
 (#match? @constant "^[A-Z][A-Z_]*$"))

; Function calls

(decorator) @function
(decorator
  (identifier) @function)

(call
  function: (attribute attribute: (identifier) @function.method))
(call
  function: (identifier) @function)

; Builtin functions

((call
  function: (identifier) @function.builtin)
 (#match?
   @function.builtin
   "^(abs|all|any|ascii|bin|bool|breakpoint|bytearray|bytes|callable|chr|classmethod|compile|complex|delattr|dict|dir|divmod|enumerate|eval|exec|filter|float|format|frozenset|getattr|globals|hasattr|hash|help|hex|id|input|int|isinstance|issubclass|iter|len|list|locals|map|max|memoryview|min|next|object|oct|open|ord|pow|print|property|range|repr|reversed|round|set|setattr|slice|sorted|staticmethod|str|sum|super|tuple|type|vars|zip|__import__)$"))

; Function definitions

(function_definition
  name: (identifier) @function)

(attribute attribute: (identifier) @property)
(type (identifier) @type)

; Literals

[
  (none)
  (true)
  (false)
] @constant.builtin

[
  (integer)
  (float)
] @number

(comment) @comment
(string) @string
(escape_sequence) @escape

(interpolation
  "{" @punctuation.special
  "}" @punctuation.special) @embedded

; TreeTerm: the `in` of a loop, and the `async` of `async for` and `async with`, are control
; keywords; the `in` of a test stays an operator, the `async` of `async def` a keyword.
(for_statement "in" @keyword.control)
(for_in_clause "in" @keyword.control)
(for_statement "async" @keyword.control)
(for_in_clause "async" @keyword.control)
(with_statement "async" @keyword.control)

[
  "-"
  "-="
  "!="
  "*"
  "**"
  "**="
  "*="
  "/"
  "//"
  "//="
  "/="
  "&"
  "&="
  "%"
  "%="
  "^"
  "^="
  "+"
  "->"
  "+="
  "<"
  "<<"
  "<<="
  "<="
  "<>"
  "="
  ":="
  "=="
  ">"
  ">="
  ">>"
  ">>="
  "|"
  "|="
  "~"
  "@="
  "and"
  "in"
  "is"
  "not"
  "or"
  "is not"
  "not in"
] @operator

[
  "async"
  "class"
  "def"
  "exec"
  "global"
  "lambda"
  "nonlocal"
  "print"
] @keyword

[
  "as"
  "assert"
  "await"
  "break"
  "continue"
  "del"
  "elif"
  "else"
  "except"
  "finally"
  "for"
  "from"
  "if"
  "import"
  "pass"
  "raise"
  "return"
  "try"
  "while"
  "with"
  "yield"
  "match"
  "case"
] @keyword.control
