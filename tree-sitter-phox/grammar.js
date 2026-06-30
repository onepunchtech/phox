/// <reference types="tree-sitter-cli/dsl" />

module.exports = grammar({
  name: "phox",

  extras: ($) => [/[ \t\r\n]/, $.comment],

  externals: ($) => [$.layout_start, $.layout_end, $.layout_semicolon],

  word: ($) => $.lower_identifier,

  conflicts: ($) => [],

  rules: {
    source_file: ($) => $._expression,

    // -----------------------------------------------------------------------
    // Comments
    // -----------------------------------------------------------------------
    comment: ($) => token(seq("--", /.*/)),

    // -----------------------------------------------------------------------
    // Identifiers
    // -----------------------------------------------------------------------
    lower_identifier: ($) => /[a-z_][a-zA-Z0-9_']*/,
    upper_identifier: ($) => /[A-Z][a-zA-Z0-9_']*/,

    // -----------------------------------------------------------------------
    // Literals
    // -----------------------------------------------------------------------
    integer: ($) => token(seq(optional("-"), /[0-9]+/)),
    double: ($) =>
      token(
        seq(optional("-"), /[0-9]+\.[0-9]+/, optional(seq(/[eE]/, optional(/[+-]/), /[0-9]+/)))
      ),
    string: ($) =>
      token(seq('"', repeat(choice(/[^"\\]/, seq("\\", /[ntr\\"'0]/))), '"')),
    char: ($) =>
      token(seq("'", choice(/[^'\\]/, seq("\\", /[ntr\\"'0]/)), "'")),
    boolean: ($) => choice("True", "False"),
    unit: ($) => "()",

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------
    _expression: ($) =>
      choice(
        $.let_expression,
        $.case_expression,
        $.if_expression,
        $.lambda,
        $.mu_type,
        $.import_expression,
        $._pipe_expression
      ),

    _pipe_expression: ($) =>
      choice($.pipe_expression, $._arrow_expression),

    pipe_expression: ($) =>
      prec.left(1, seq($._arrow_expression, "|>", $._pipe_expression)),

    _arrow_expression: ($) =>
      choice($.arrow_type, $.pi_type, $._application_expression),

    arrow_type: ($) =>
      prec.right(2, seq($._application_expression, "->", $._pipe_expression)),

    pi_type: ($) =>
      prec.right(
        1,
        seq(
          "(",
          field("param", $.lower_identifier),
          ":",
          field("param_type", $._expression),
          ")",
          "->",
          field("return_type", $._expression)
        )
      ),

    _application_expression: ($) => choice($.application, $._access_expression),

    application: ($) =>
      prec.left(
        2,
        seq(
          field("function", $._access_expression),
          repeat1(field("argument", $._argument))
        )
      ),

    _argument: ($) => choice($._access_expression, $.implicit_argument),

    implicit_argument: ($) => seq("?{", $._expression, "}"),

    _access_expression: ($) => choice($.record_access, $._atom),

    record_access: ($) =>
      prec.left(3, seq($._access_expression, ".", field("field", $.lower_identifier))),

    _atom: ($) =>
      choice(
        $.lower_identifier,
        $.upper_identifier,
        $.integer,
        $.double,
        $.string,
        $.char,
        $.boolean,
        $.unit,
        $.type_builtin,
        $.undefined,
        $.hole,
        $.variant_constructor,
        $.fold_expression,
        $.unfold_expression,
        $.record_literal,
        $.record_update,
        $.record_type,
        $.rec_type,
        $.variant_type,
        $.list_literal,
        $.parenthesized,
        $.type_annotation
      ),

    type_builtin: ($) =>
      choice("Type", "String", "Integer", "Double", "Char", "Bool", "Unit", "Row"),

    undefined: ($) => "undefined",

    hole: ($) => seq("?", $.lower_identifier),

    variant_constructor: ($) =>
      prec.left(2, seq("'", field("tag", $.upper_identifier), optional(field("payload", $._access_expression)))),

    fold_expression: ($) => prec(2, seq("fold", $._access_expression)),

    unfold_expression: ($) => prec(2, seq("unfold", $._access_expression)),

    parenthesized: ($) => seq("(", $._expression, ")"),

    type_annotation: ($) =>
      seq("(", $._expression, ":", $._expression, ")"),

    // -----------------------------------------------------------------------
    // Records
    // -----------------------------------------------------------------------
    record_literal: ($) =>
      seq(
        "{",
        optional(
          seq(
            $.record_field,
            repeat(seq(",", $.record_field)),
            optional(",")
          )
        ),
        "}"
      ),

    record_update: ($) =>
      seq(
        "{",
        "...",
        field("base", $._expression),
        optional(seq(",", $.record_field, repeat(seq(",", $.record_field)))),
        optional(","),
        "}"
      ),

    record_field: ($) =>
      seq(
        field("name", $.lower_identifier),
        "=",
        field("value", $._expression)
      ),

    record_type: ($) =>
      seq(
        "{",
        $.record_type_field,
        repeat(seq(",", $.record_type_field)),
        optional(seq(";", field("tail", $._expression))),
        "}"
      ),

    rec_type: ($) =>
      seq(
        "Rec",
        "{",
        $.record_type_field,
        repeat(seq(",", $.record_type_field)),
        optional(seq(";", field("tail", $._expression))),
        "}"
      ),

    record_type_field: ($) =>
      seq(
        field("name", $.lower_identifier),
        ":",
        field("type", $._expression)
      ),

    // -----------------------------------------------------------------------
    // Variants
    // -----------------------------------------------------------------------
    variant_type: ($) =>
      seq(
        "<",
        optional(
          seq(
            $.variant_tag,
            repeat(seq("|", $.variant_tag)),
            optional(seq(";", field("tail", $._expression)))
          )
        ),
        ">"
      ),

    variant_tag: ($) =>
      seq(
        "'",
        field("tag", $.upper_identifier),
        optional(field("payload", $._access_expression))
      ),

    // -----------------------------------------------------------------------
    // Lists
    // -----------------------------------------------------------------------
    list_literal: ($) =>
      seq(
        "[",
        optional(seq($._expression, repeat(seq(",", $._expression)))),
        "]"
      ),

    // -----------------------------------------------------------------------
    // Let expression
    // -----------------------------------------------------------------------
    let_expression: ($) =>
      seq(
        "let",
        $.layout_start,
        $.let_binding,
        repeat(seq($.layout_semicolon, $.let_binding)),
        $.layout_end,
        "in",
        field("body", $._expression)
      ),

    let_binding: ($) =>
      choice(
        // Inline typed: name : type = value
        seq(
          field("name", $._binding_name),
          ":",
          field("type", $._binding_type),
          "=",
          field("value", $._expression)
        ),
        // Two-line typed: name : type \n name params = value
        seq(
          field("name", $._binding_name),
          ":",
          field("type", $._binding_type),
          field("name", $._binding_name),
          repeat(field("param", $._pattern)),
          "=",
          field("value", $._expression)
        ),
        // Untyped binding: name params = value
        seq(
          field("name", $._binding_name),
          repeat(field("param", $._pattern)),
          "=",
          field("value", $._expression)
        )
      ),

    _binding_name: ($) => choice($.lower_identifier, $.upper_identifier),

    _binding_type: ($) => $._arrow_expression,

    // -----------------------------------------------------------------------
    // Case expression
    // -----------------------------------------------------------------------
    case_expression: ($) =>
      seq(
        "case",
        field("scrutinee", $._expression),
        "of",
        $.layout_start,
        $.case_branch,
        repeat(seq($.layout_semicolon, $.case_branch)),
        $.layout_end,
      ),

    case_branch: ($) =>
      seq(
        field("pattern", $._pattern),
        "=>",
        field("body", $._expression)
      ),

    // -----------------------------------------------------------------------
    // If expression
    // -----------------------------------------------------------------------
    if_expression: ($) =>
      seq(
        "if",
        field("condition", $._expression),
        "then",
        field("then", $._expression),
        "else",
        field("else", $._expression)
      ),

    // -----------------------------------------------------------------------
    // Lambda
    // -----------------------------------------------------------------------
    lambda: ($) =>
      seq("\\", repeat1(field("param", $._pattern)), "=>", field("body", $._expression)),

    // -----------------------------------------------------------------------
    // Mu type
    // -----------------------------------------------------------------------
    mu_type: ($) =>
      seq("Mu", field("var", $.lower_identifier), ".", field("body", $._expression)),

    // -----------------------------------------------------------------------
    // Import
    // -----------------------------------------------------------------------
    import_expression: ($) => seq("import", field("path", $.string)),

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------
    _pattern: ($) =>
      choice(
        $.pattern_variable,
        $.pattern_wildcard,
        $.pattern_literal,
        $.pattern_variant,
        $.pattern_record,
        $.pattern_parenthesized
      ),

    pattern_variable: ($) => $.lower_identifier,

    pattern_wildcard: ($) => "_",

    pattern_literal: ($) =>
      choice($.integer, $.string, $.char, $.boolean, $.unit),

    pattern_variant: ($) =>
      prec.left(2, seq("'", field("tag", $.upper_identifier), optional(field("payload", $._pattern)))),

    pattern_record: ($) =>
      seq(
        "{",
        optional(
          seq(
            $._record_pattern_field,
            repeat(seq(",", $._record_pattern_field))
          )
        ),
        "}"
      ),

    _record_pattern_field: ($) =>
      choice($.record_pattern_pun, $.record_pattern_match),

    record_pattern_pun: ($) => $.lower_identifier,

    record_pattern_match: ($) =>
      seq(field("name", $.lower_identifier), "=", field("pattern", $._pattern)),

    pattern_parenthesized: ($) => seq("(", $._pattern, ")"),
  },
});
