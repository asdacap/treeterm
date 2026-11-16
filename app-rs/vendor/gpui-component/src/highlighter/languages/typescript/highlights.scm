; Types
; Variables
;----------

(identifier) @variable

; Properties
;-----------

(property_identifier) @property
(shorthand_property_identifier) @property
(shorthand_property_identifier_pattern) @property
(private_property_identifier) @property

; Function and method definitions
;--------------------------------

(function_expression
  name: (identifier) @function)
(function_declaration
  name: (identifier) @function)
(method_definition
  name: (property_identifier) @function.method)

(pair
  key: (property_identifier) @function.method
  value: [(function_expression) (arrow_function)])

(assignment_expression
  left: (member_expression
    property: (property_identifier) @function.method)
  right: [(function_expression) (arrow_function)])

(variable_declarator
  name: (identifier) @function
  value: [(function_expression) (arrow_function)])

(assignment_expression
  left: (identifier) @function
  right: [(function_expression) (arrow_function)])

; Function and method calls
;--------------------------

(call_expression
  function: (identifier) @function)

(call_expression
  function: (member_expression
    property: (property_identifier) @function.method))

; Special identifiers
;--------------------

((identifier) @type
 (#match? @type "^[A-Z]"))

([
  (identifier)
  (shorthand_property_identifier)
  (shorthand_property_identifier_pattern)
 ] @constant
 (#match? @constant "^_*[A-Z_][A-Z\\d_]*$"))

((identifier) @variable.builtin
 (#match? @variable.builtin "^(arguments|module|console|window|document)$")
 (#is-not? local))

((identifier) @function.builtin
 (#eq? @function.builtin "require")
 (#is-not? local))

; Literals
;---------

; TreeTerm patch: `this` and `super` are keywords, as Monaco's and VS Code's Dark+ colour
; them (`@variable.builtin` falls back to `@variable`).
(this) @variable.special
(super) @variable.special

[
  (true)
  (false)
  (null)
  (undefined)
] @constant.builtin

(comment) @comment

[
  (string)
  (template_string)
] @string

(regex) @string.special
(number) @number

; Tokens
;-------

[
  ";"
  (optional_chain)
  "."
  ","
] @punctuation.delimiter

[
  "-"
  "--"
  "-="
  "+"
  "++"
  "+="
  "*"
  "*="
  "**"
  "**="
  "/"
  "/="
  "%"
  "%="
  "<"
  "<="
  "<<"
  "<<="
  "="
  "=="
  "==="
  "!"
  "!="
  "!=="
  "=>"
  ">"
  ">="
  ">>"
  ">>="
  ">>>"
  ">>>="
  "~"
  "^"
  "&"
  "|"
  "^="
  "&="
  "|="
  "&&"
  "||"
  "??"
  "&&="
  "||="
  "??="
] @operator

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
]  @punctuation.bracket

(template_substitution
  "${" @punctuation.special
  "}" @punctuation.special) @embedded

[
  "async"
  "class"
  "const"
  "debugger"
  "delete"
  "extends"
  "function"
  "get"
  "in"
  "instanceof"
  "let"
  "new"
  "of"
  "set"
  "static"
  "target"
  "typeof"
  "var"
  "void"
] @keyword

; TreeTerm patch: control-flow and module keywords, which VS Code's grammar scopes
; `keyword.control.*` (`.flow`, `.loop`, `.import`, `.export`, …) and Dark+ colours apart.
[
  "as"
  "await"
  "break"
  "case"
  "catch"
  "continue"
  "default"
  "do"
  "else"
  "export"
  "finally"
  "for"
  "from"
  "if"
  "import"
  "return"
  "switch"
  "throw"
  "try"
  "while"
  "with"
  "yield"
] @keyword.control


(type_identifier) @type
(predefined_type) @type.builtin

((identifier) @type
 (#match? @type "^[A-Z]"))

(type_arguments
  "<" @punctuation.bracket
  ">" @punctuation.bracket)

; Variables

(required_parameter (identifier) @variable.parameter)
(optional_parameter (identifier) @variable.parameter)

; Keywords

; TreeTerm patch: `satisfies`, `type` in an import or export and `require` in
; `import x = require(…)` are control keywords, as VS Code's TypeScript grammar scopes them
; (`keyword.control.satisfies.ts`, `keyword.control.type.ts`, `keyword.control.require.ts`);
; `export` is one already (above).
"satisfies" @keyword.control
(import_statement "type" @keyword.control)
(export_statement "type" @keyword.control)
(import_specifier "type" @keyword.control)
(import_require_clause "require" @keyword.control)

[ "abstract"
  "declare"
  "enum"
  "implements"
  "interface"
  "keyof"
  "namespace"
  "private"
  "protected"
  "public"
  "type"
  "readonly"
  "override"
] @keyword
