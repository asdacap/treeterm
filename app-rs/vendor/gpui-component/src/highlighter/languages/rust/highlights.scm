; Identifiers

(type_identifier) @type
(primitive_type) @type.builtin
(field_identifier) @property

; Identifier conventions

; Assume all-caps names are constants
((identifier) @constant
 (#match? @constant "^[A-Z][A-Z\\d_]+$"))

; Assume uppercase names are enum constructors
((identifier) @constructor
 (#match? @constructor "^[A-Z]"))

; Assume that uppercase names in paths are types
((scoped_identifier
  path: (identifier) @type)
 (#match? @type "^[A-Z]"))
((scoped_identifier
  path: (scoped_identifier
    name: (identifier) @type))
 (#match? @type "^[A-Z]"))
((scoped_type_identifier
  path: (identifier) @type)
 (#match? @type "^[A-Z]"))
((scoped_type_identifier
  path: (scoped_identifier
    name: (identifier) @type))
 (#match? @type "^[A-Z]"))

; TreeTerm patch: the other names in a path are modules (`std` and `sync` in
; `std::sync::Arc`, `tracing` in `tracing::error!`), as VS Code's Dark+ colours them
; (`entity.name.namespace`). After the rules above, so that `Vec` in `Vec::new` stays a
; type: of two captures of one range, the first match's wins, and matches that start at
; the same node come in pattern order.
(scoped_identifier
  path: (identifier) @namespace.path)
(scoped_identifier
  path: (scoped_identifier
    name: (identifier) @namespace.path))
(scoped_type_identifier
  path: (identifier) @namespace.path)
(scoped_type_identifier
  path: (scoped_identifier
    name: (identifier) @namespace.path))
(scoped_use_list
  path: (identifier) @namespace.path)
(scoped_use_list
  path: (scoped_identifier
    name: (identifier) @namespace.path))
(use_wildcard
  (identifier) @namespace.path)
(use_wildcard
  (scoped_identifier
    name: (identifier) @namespace.path))

; TreeTerm patch: what a `use` or `extern crate` brings in, a module's name and an
; attribute's words are plain text (`@primary`), as Dark+ and Monaco leave them, not the
; variables of the last rule. These matches start at an enclosing node, so they come before
; that rule's; the lowercase test leaves `Arc` in `use std::sync::Arc` and `Debug` in
; `#[derive(Debug)]` to the uppercase rules above.
((use_declaration
  argument: (identifier) @primary)
 (#match? @primary "^[a-z_]"))
((use_declaration
  argument: (scoped_identifier
    name: (identifier) @primary))
 (#match? @primary "^[a-z_]"))
((use_list
  (identifier) @primary)
 (#match? @primary "^[a-z_]"))
((use_list
  (scoped_identifier
    name: (identifier) @primary))
 (#match? @primary "^[a-z_]"))
((use_as_clause
  path: (identifier) @primary)
 (#match? @primary "^[a-z_]"))
((use_as_clause
  path: (scoped_identifier
    name: (identifier) @primary))
 (#match? @primary "^[a-z_]"))
((use_as_clause
  alias: (identifier) @primary)
 (#match? @primary "^[a-z_]"))
(extern_crate_declaration
  name: (identifier) @primary)
(extern_crate_declaration
  alias: (identifier) @primary)
(mod_item
  name: (identifier) @primary)
(attribute
  (identifier) @primary)
(attribute
  (scoped_identifier
    path: (identifier) @primary))
(attribute
  (scoped_identifier
    name: (identifier) @primary))
((attribute
  arguments: (token_tree
    (identifier) @primary))
 (#match? @primary "^[a-z_]"))
((attribute
  arguments: (token_tree
    (token_tree
      (identifier) @primary)))
 (#match? @primary "^[a-z_]"))
((attribute
  arguments: (token_tree
    (token_tree
      (token_tree
        (identifier) @primary))))
 (#match? @primary "^[a-z_]"))

; Assume all qualified names in struct patterns are enum constructors. (They're
; either that, or struct names; highlighting both as constructors seems to be
; the less glaring choice of error, visually.)
(struct_pattern
  type: (scoped_type_identifier
    name: (type_identifier) @constructor))

; Function calls

(call_expression
  function: (identifier) @function)
(call_expression
  function: (field_expression
    field: (field_identifier) @function.method))
(call_expression
  function: (scoped_identifier
    "::"
    name: (identifier) @function))

(generic_function
  function: (identifier) @function)
(generic_function
  function: (scoped_identifier
    name: (identifier) @function))
(generic_function
  function: (field_expression
    field: (field_identifier) @function.method))

; TreeTerm patch: one rule for the name, one for the `!`, so the name's match completes at
; the name, ahead of the `(identifier) @variable` rule at the end (of two captures of one
; range, the first match's wins).
(macro_invocation
  macro: (identifier) @function.macro)
; TreeTerm patch: a macro named by its path (`error` in `tracing::error!`), and the name a
; `macro_rules!` defines, as Dark+ colours them.
(macro_invocation
  macro: (scoped_identifier
    name: (identifier) @function.macro))
(macro_invocation
  "!" @function.macro)
(macro_definition
  name: (identifier) @function.macro)

; Function definitions

(function_item (identifier) @function)
(function_signature_item (identifier) @function)

[
  (line_comment)
  (block_comment)
] @comment

[
  (line_comment (doc_comment))
  (block_comment (doc_comment))
] @comment.doc

"(" @punctuation.bracket
")" @punctuation.bracket
"[" @punctuation.bracket
"]" @punctuation.bracket
"{" @punctuation.bracket
"}" @punctuation.bracket

(type_arguments
  "<" @punctuation.bracket
  ">" @punctuation.bracket)
(type_parameters
  "<" @punctuation.bracket
  ">" @punctuation.bracket)

"::" @punctuation.delimiter
":" @punctuation.delimiter
"." @punctuation.delimiter
"," @punctuation.delimiter
";" @punctuation.delimiter

(parameter (identifier) @variable.parameter)

(lifetime (identifier) @label)

"as" @keyword
"async" @keyword
"await" @keyword
"break" @keyword
"const" @keyword
"continue" @keyword
"default" @keyword
"dyn" @keyword
"else" @keyword
"enum" @keyword
"extern" @keyword
"fn" @keyword
"for" @keyword
"gen" @keyword
"if" @keyword
"impl" @keyword
"in" @keyword
"let" @keyword
"loop" @keyword
"macro_rules!" @keyword
"match" @keyword
"mod" @keyword
"move" @keyword
"pub" @keyword
"raw" @keyword
"ref" @keyword
"return" @keyword
"static" @keyword
"struct" @keyword
"trait" @keyword
"type" @keyword
"union" @keyword
"unsafe" @keyword
"use" @keyword
"where" @keyword
"while" @keyword
"yield" @keyword
(crate) @keyword
(mutable_specifier) @keyword
(use_list (self) @keyword)
(scoped_use_list (self) @keyword)
(scoped_identifier (self) @keyword)
(super) @keyword

; TreeTerm patch: `self` is a keyword, as Monaco's Rust grammar and VS Code's Dark+ colour it
; (`@variable.builtin` falls back to `@variable`).
(self) @variable.special

(char_literal) @string
(string_literal) @string
(raw_string_literal) @string

(boolean_literal) @constant.builtin
; TreeTerm patch: numbers are `@number` and escapes `@string.escape`, as Monaco's Rust
; grammar and VS Code's tokenise them (`@constant.builtin` is `true`'s colour, and no theme
; colours `@escape`).
(integer_literal) @number
(float_literal) @number

(escape_sequence) @string.escape

; TreeTerm patch: attributes are plain text, as Dark+ and Monaco show them (`@attribute` is
; an HTML attribute's colour); the strings and type names in them keep theirs.
(attribute_item) @primary
(inner_attribute_item) @primary

"*" @operator
"&" @operator
"'" @operator

; TreeTerm patch: the identifiers no rule above captured are variables, as VS Code's Dark+
; colours them. Last, so that the other rules' captures of an identifier win: a match that
; completes at the identifier comes before this one, and of two captures of one range the
; first match's wins.
(identifier) @variable
