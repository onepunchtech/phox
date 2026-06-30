; Keywords
[
  "let" "in" "case" "of" "if" "then" "else" "import"
  "fold" "unfold" "Mu" "Rec"
] @keyword

(undefined) @keyword

; Type builtins
(type_builtin) @type.builtin

; Booleans
(boolean) @boolean

; Literals
(integer) @number
(double) @number.float
(string) @string
(char) @character
(unit) @constant

; Identifiers
(lower_identifier) @variable
(upper_identifier) @type

; Variant constructors
(variant_constructor
  tag: (upper_identifier) @constructor)

(variant_tag
  tag: (upper_identifier) @constructor)

(pattern_variant
  tag: (upper_identifier) @constructor)

; Function definitions in let bindings
(let_binding
  name: (lower_identifier) @function)
(let_binding
  name: (upper_identifier) @function)

; Lambda parameters
(lambda
  param: (pattern_variable
    (lower_identifier) @variable.parameter))

; Holes
(hole) @attribute

; Operators and punctuation
["=>" "->" "=" ":" "." "|" "\\" "|>" "..."] @operator
["(" ")" "{" "}" "[" "]" "<" ">"] @punctuation.bracket
["," ";"] @punctuation.delimiter

; Wildcard
(pattern_wildcard) @variable.builtin

; Comments
(comment) @comment
