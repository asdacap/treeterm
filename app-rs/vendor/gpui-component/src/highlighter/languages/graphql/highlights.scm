; TreeTerm: tree-sitter-graphql 0.1.0 ships no queries; this one is written against its
; `node-types.json`, colouring what Monaco's GraphQL grammar colours.

(comment) @comment

(string_value) @string

[
  (int_value)
  (float_value)
] @number

(boolean_value) @boolean

(null_value) @constant

(operation_type) @keyword

[
  "query"
  "mutation"
  "subscription"
  "fragment"
  "on"
  "type"
  "interface"
  "union"
  "enum"
  "input"
  "scalar"
  "schema"
  "extend"
  "implements"
  "directive"
  "repeatable"
] @keyword

(named_type (name) @type)
(object_type_definition (name) @type)
(interface_type_definition (name) @type)
(enum_type_definition (name) @type)
(union_type_definition (name) @type)
(input_object_type_definition (name) @type)
(scalar_type_definition (name) @type)

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket
