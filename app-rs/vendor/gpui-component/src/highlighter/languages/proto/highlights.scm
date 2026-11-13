; From tree-sitter-proto 0.2.0 (MIT), whose Rust bindings do not export it.
; TreeTerm: adds `syntax = "proto3";`'s version, a token in the grammar, not a `(string)`.

[
  "syntax"
  "edition"
  "package"
  "option"
  "import"
  "service"
  "rpc"
  "returns"
  "message"
  "enum"
  "oneof"
  "repeated"
  "reserved"
  "to"
] @keyword

[
  (key_type)
  (type)
  (message_name)
  (enum_name)
  (service_name)
  (rpc_name)
]@type

[
  (string)
  "\"proto2\""
  "\"proto3\""
] @string

[
  (int_lit)
  (float_lit)
] @number

[
  (true)
  (false)
] @constant.builtin

(comment) @comment

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
]  @punctuation.bracket

