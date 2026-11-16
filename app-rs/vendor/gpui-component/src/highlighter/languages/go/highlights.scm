; Function calls

(call_expression
  function: (identifier) @function)

(call_expression
  function: (identifier) @function.builtin
  (#match? @function.builtin "^(append|cap|close|complex|copy|delete|imag|len|make|new|panic|print|println|real|recover)$"))

(call_expression
  function: (selector_expression
    field: (field_identifier) @function.method))

; Function definitions

(function_declaration
  name: (identifier) @function)

(method_declaration
  name: (field_identifier) @function.method)
(method_elem
  name: (field_identifier) @function.method)

; Identifiers
(keyed_element
  .
  (literal_element
    (identifier) @variable.member))

(type_identifier) @type
(field_identifier) @variable.member
(identifier) @variable
(package_identifier) @namespace

; Operators

[
  "--"
  "-"
  "-="
  ":="
  "!"
  "!="
  "..."
  "*"
  "*"
  "*="
  "/"
  "/="
  "&"
  "&&"
  "&="
  "%"
  "%="
  "^"
  "^="
  "+"
  "++"
  "+="
  "<-"
  "<"
  "<<"
  "<<="
  "<="
  "="
  "=="
  ">"
  ">="
  ">>"
  ">>="
  "|"
  "|="
  "||"
  "~"
] @operator

; Keywords

[
  "chan"
  "const"
  "func"
  "interface"
  "map"
  "package"
  "struct"
  "type"
  "var"
] @keyword

; TreeTerm patch: control-flow keywords and `import`, which VS Code's Go grammar scopes
; `keyword.control.go` and `keyword.control.import.go` and Dark+ colours apart.
[
  "break"
  "case"
  "continue"
  "default"
  "defer"
  "else"
  "fallthrough"
  "for"
  "go"
  "goto"
  "if"
  "import"
  "range"
  "return"
  "select"
  "switch"
] @keyword.control

; Literals

[
  (interpreted_string_literal)
  (raw_string_literal)
  (rune_literal)
] @string

(escape_sequence) @string.escape

[
  (int_literal)
  (float_literal)
  (imaginary_literal)
] @number

(const_spec
  name: (identifier) @constant)

[
  (true)
  (false)
] @boolean

[
  (nil)
  (iota)
] @constant.builtin

(comment) @comment
