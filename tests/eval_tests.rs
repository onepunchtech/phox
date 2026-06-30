/// End-to-end evaluation tests.
/// Each test parses a .px expression, elaborates, evaluates, and checks the result.
use phox::elaborate;
use phox::elaborate::{MetaCxt, TermPrinter};

fn eval_to_string(input: &str) -> Result<(String, String), String> {
    let expr = phox::parser::parse(input).map_err(|errs| {
        errs.iter()
            .map(|e| format!("{e}"))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let mcxt = MetaCxt::new();
    let cxt = elaborate::Cxt::new(&mcxt);

    let (tm, ty) = elaborate::infer(&cxt, &expr).map_err(|e| format!("{e}"))?;
    let nf = elaborate::nf(&mcxt, &cxt.env, &tm).map_err(|e| format!("{e}"))?;
    let ty_tm = elaborate::quote(&mcxt, cxt.lvl, &ty);
    let ty_nf = elaborate::nf(&mcxt, &cxt.env, &ty_tm).unwrap_or(ty_tm);

    Ok((format!("{}", TermPrinter(&nf)), format!("{}", TermPrinter(&ty_nf))))
}

fn assert_eval(input: &str, expected_val: &str, expected_ty: &str) {
    match eval_to_string(input) {
        Ok((val, ty)) => {
            assert_eq!(val, expected_val, "value mismatch for: {input}");
            assert_eq!(ty, expected_ty, "type mismatch for: {input}");
        }
        Err(e) => panic!("eval failed for: {input}\nerror: {e}"),
    }
}

fn assert_eval_fails(input: &str) {
    assert!(eval_to_string(input).is_err(), "expected failure for: {input}");
}

fn assert_eval_val(input: &str, expected_val: &str) {
    match eval_to_string(input) {
        Ok((val, _)) => {
            assert_eq!(val, expected_val, "value mismatch for: {input}");
        }
        Err(e) => panic!("eval failed for: {input}\nerror: {e}"),
    }
}

// === Literals ===

#[test]
fn test_integer() {
    assert_eval("42", "42", "Integer");
}

#[test]
fn test_negative_integer() {
    assert_eval("-7", "-7", "Integer");
}

#[test]
fn test_string() {
    assert_eval("\"hello\"", "\"hello\"", "String");
}

#[test]
fn test_bool_true() {
    assert_eval("True", "True", "Bool");
}

#[test]
fn test_bool_false() {
    assert_eval("False", "False", "Bool");
}

#[test]
fn test_unit() {
    assert_eval("()", "()", "Unit");
}

#[test]
fn test_char() {
    assert_eval("'x'", "'x'", "Char");
}

// === Type literals ===

#[test]
fn test_type_literal() {
    assert_eval("Integer", "Integer", "Type");
}

#[test]
fn test_type_of_type() {
    assert_eval("Type", "Type", "Type");
}

// === Let bindings ===

#[test]
fn test_let_simple() {
    assert_eval("let x = 5 in x", "5", "Integer");
}

#[test]
fn test_let_typed() {
    assert_eval("let\n  x : Integer\n  x = 42\nin x", "42", "Integer");
}

#[test]
fn test_let_multiple() {
    assert_eval("let\n  x = 1\n  y = 2\nin y", "2", "Integer");
}

#[test]
fn test_let_function() {
    assert_eval(
        "let f x = x in f 42",
        "42",
        "Integer",
    );
}

// === Lambda & Application ===

#[test]
fn test_identity() {
    assert_eval(
        "let\n  id : (a : Type) -> a -> a\n  id _ x = x\nin id Integer 5",
        "5",
        "Integer",
    );
}

#[test]
fn test_const_fn() {
    assert_eval(
        "let\n  const : (a : Type) -> (b : Type) -> a -> b -> a\n  const _ _ x _ = x\nin const String Integer \"hello\" 42",
        "\"hello\"",
        "String",
    );
}

#[test]
fn test_higher_order() {
    assert_eval(
        "let\n  apply : (a : Type) -> (b : Type) -> (a -> b) -> a -> b\n  apply _ _ f x = f x\n  double : Integer -> Integer\n  double x = x\nin apply Integer Integer double 21",
        "21",
        "Integer",
    );
}

// === If-then-else ===

#[test]
fn test_if_true() {
    assert_eval("if True then 1 else 0", "1", "Integer");
}

#[test]
fn test_if_false() {
    assert_eval("if False then \"yes\" else \"no\"", "\"no\"", "String");
}

// === Records ===

#[test]
fn test_record_literal() {
    assert_eval_val("{x = 1, y = 2}", "{x = 1, y = 2}");
}

#[test]
fn test_record_access() {
    assert_eval("let r = {name = \"Alice\", age = 30} in r.name", "\"Alice\"", "String");
}

#[test]
fn test_record_access_age() {
    assert_eval("let r = {name = \"Alice\", age = 30} in r.age", "30", "Integer");
}

#[test]
fn test_nested_record_access() {
    assert_eval(
        "let\n  addr = {street = \"123 Main\", city = \"NYC\"}\n  person = {name = \"Alice\", address = addr}\nin person.address.city",
        "\"NYC\"",
        "String",
    );
}

#[test]
fn test_record_typed() {
    assert_eval(
        "let\n  Person : Type\n  Person = Rec {name : String, age : Integer}\n  alice : Person\n  alice = {name = \"Alice\", age = 30}\nin alice.age",
        "30",
        "Integer",
    );
}

// === Variants ===

#[test]
fn test_variant_constructor() {
    assert_eval_val("'Just 42", "'Just 42");
}

#[test]
fn test_variant_nullary() {
    assert_eval_val("'Nothing", "'Nothing");
}

#[test]
fn test_variant_case() {
    assert_eval(
        "let\n  Maybe : Type -> Type\n  Maybe t = < 'Nothing | 'Just t >\n  fromMaybe : (a : Type) -> a -> Maybe a -> a\n  fromMaybe _ default val = case val of\n    'Just x => x\n    'Nothing => default\nin fromMaybe Integer 0 ('Just 42)",
        "42",
        "Integer",
    );
}

#[test]
fn test_variant_case_default() {
    assert_eval(
        "let\n  Maybe : Type -> Type\n  Maybe t = < 'Nothing | 'Just t >\n  fromMaybe : (a : Type) -> a -> Maybe a -> a\n  fromMaybe _ default val = case val of\n    'Just x => x\n    'Nothing => default\nin fromMaybe Integer 0 ('Nothing)",
        "0",
        "Integer",
    );
}

// === Case on literals ===

#[test]
fn test_case_literal() {
    assert_eval(
        "let\n  greet : String -> String\n  greet name = case name of\n    \"Alice\" => \"Hello Alice!\"\n    \"Bob\" => \"Hey Bob!\"\n    _ => \"Hi stranger!\"\nin greet \"Alice\"",
        "\"Hello Alice!\"",
        "String",
    );
}

#[test]
fn test_case_literal_wildcard() {
    assert_eval(
        "let\n  greet : String -> String\n  greet name = case name of\n    \"Alice\" => \"Hello Alice!\"\n    _ => \"Hi stranger!\"\nin greet \"Charlie\"",
        "\"Hi stranger!\"",
        "String",
    );
}

// === Full example from test.px ===

#[test]
fn test_full_example() {
    assert_eval(
        "\
let
  Either : Type -> Type -> Type
  Either l r = < 'Left l | 'Right r >

  Maybe : Type -> Type
  Maybe t = < 'Nothing | 'Just t >

  fromMaybe : (a : Type) -> a -> Maybe a -> a
  fromMaybe _ default val = case val of
    'Just x => x
    'Nothing => default

  result : Either String Integer
  result = 'Right 42
in fromMaybe Integer 0 ('Just 5)",
        "5",
        "Integer",
    );
}

// === Mu / fold / unfold (isorecursive types) ===

#[test]
fn test_mu_nat_zero() {
    assert_eval(
        "let\n  Nat : Type\n  Nat = Mu self. < 'Zero | 'Succ self >\n  zero : Nat\n  zero = fold 'Zero\nin zero",
        "fold 'Zero",
        "Mu _. < 'Zero | 'Succ #0 >",
    );
}

#[test]
fn test_mu_nat_succ() {
    assert_eval(
        "let\n  Nat : Type\n  Nat = Mu self. < 'Zero | 'Succ self >\n  zero : Nat\n  zero = fold 'Zero\n  one : Nat\n  one = fold ('Succ zero)\nin one",
        "fold 'Succ (fold 'Zero)",
        "Mu _. < 'Zero | 'Succ #0 >",
    );
}

#[test]
fn test_mu_unfold_nat() {
    assert_eval(
        "let\n  Nat : Type\n  Nat = Mu self. < 'Zero | 'Succ self >\n  isZero : Nat -> Bool\n  isZero n = case unfold n of\n    'Zero => True\n    'Succ _ => False\nin isZero (fold 'Zero)",
        "True",
        "Bool",
    );
}

#[test]
fn test_mu_linked_list_head() {
    assert_eval(
        "\
let
  List : Type -> Type
  List a = Mu self. < 'Nil | 'Cons Rec {head : a, tail : self} >
  head : (a : Type) -> a -> List a -> a
  head _ default xs = case unfold xs of
    'Cons r => r.head
    'Nil => default
  myList : List Integer
  myList = fold ('Cons {head = 1, tail = fold ('Cons {head = 2, tail = fold 'Nil})})
in head Integer 0 myList",
        "1",
        "Integer",
    );
}

#[test]
fn test_mu_linked_list_tail() {
    assert_eval(
        "\
let
  List : Type -> Type
  List a = Mu self. < 'Nil | 'Cons Rec {head : a, tail : self} >
  head : (a : Type) -> a -> List a -> a
  head _ default xs = case unfold xs of
    'Cons r => r.head
    'Nil => default
  tail : (a : Type) -> List a -> List a
  tail a xs = case unfold xs of
    'Cons r => r.tail
    'Nil => fold 'Nil
  myList : List Integer
  myList = fold ('Cons {head = 1, tail = fold ('Cons {head = 2, tail = fold 'Nil})})
in head Integer 0 (tail Integer myList)",
        "2",
        "Integer",
    );
}

#[test]
fn test_mu_type_is_type() {
    assert_eval(
        "Mu self. < 'Nil | 'Cons self >",
        "Mu _. < 'Nil | 'Cons #0 >",
        "Type",
    );
}

#[test]
fn test_mu_fold_cant_infer() {
    assert_eval_fails("fold 42");
}

// === Phox high-level API ===

#[test]
fn test_phox_eval() {
    let phox = phox::Phox::new();
    let result = phox.eval("let x = 42 in x").unwrap();
    assert_eq!(format!("{}", phox::TermPrinter(&result.term)), "42");
    assert_eq!(format!("{}", phox::TermPrinter(&result.ty_term)), "Integer");
}

#[test]
fn test_phox_eval_checked() {
    let phox = phox::Phox::new();
    let result = phox
        .eval_checked(
            r#"{ name = "my-app", port = 8080 }"#,
            "Rec { name : String, port : Integer }",
        )
        .unwrap();
    assert_eq!(
        format!("{}", phox::TermPrinter(&result.term)),
        r#"{name = "my-app", port = 8080}"#
    );
}

#[test]
fn test_phox_eval_checked_type_mismatch() {
    let phox = phox::Phox::new();
    assert!(phox.eval_checked("42", "String").is_err());
}

// === Inline typed let bindings ===

#[test]
fn test_inline_typed_let() {
    assert_eval("let x : Integer = 42 in x", "42", "Integer");
}

#[test]
fn test_inline_typed_let_record() {
    assert_eval(
        "let\n  Config : Type = Rec { name : String, port : Integer }\n  c : Config = { name = \"app\", port = 80 }\nin c.port",
        "80",
        "Integer",
    );
}

// === Auto-implicit quantification ===

#[test]
fn test_auto_implicit_id() {
    assert_eval(
        "let\n  id : a -> a\n  id x = x\nin id 5",
        "5",
        "Integer",
    );
}

#[test]
fn test_auto_implicit_const() {
    assert_eval(
        "let\n  const : a -> b -> a\n  const x _ = x\nin const 1 \"hello\"",
        "1",
        "Integer",
    );
}

#[test]
fn test_auto_implicit_higher_order() {
    assert_eval(
        "let\n  apply : (a -> b) -> a -> b\n  apply f x = f x\nin apply (\\x => x) 42",
        "42",
        "Integer",
    );
}

#[test]
fn test_auto_implicit_in_scope_not_quantified() {
    // `a` is already bound as Integer, should not be auto-quantified
    assert_eval(
        "let\n  a : Type = Integer\n  id : a -> a\n  id x = x\nin id 5",
        "5",
        "Integer",
    );
}

#[test]
fn test_auto_implicit_maybe() {
    assert_eval(
        "\
let
  Maybe : Type -> Type
  Maybe t = < 'Nothing | 'Just t >
  fromMaybe : a -> Maybe a -> a
  fromMaybe default val = case val of
    'Just x => x
    'Nothing => default
in fromMaybe 0 ('Just 42)",
        "42",
        "Integer",
    );
}

// === Record spread/update ===

#[test]
fn test_record_spread_override() {
    assert_eval(
        "let base = {x = 1, y = 2} in {...base, x = 10}",
        "{x = 10, y = 2}",
        "Rec {x : Integer, y : Integer}",
    );
}

#[test]
fn test_record_spread_no_override() {
    assert_eval(
        "let base = {x = 1, y = 2} in {...base}",
        "{x = 1, y = 2}",
        "Rec {x : Integer, y : Integer}",
    );
}

#[test]
fn test_record_spread_checked() {
    assert_eval(
        "\
let
  Config : Type = Rec { name : String, port : Integer, debug : Bool }
  defaults : Config = { name = \"app\", port = 8080, debug = False }
  mine : Config = { ...defaults, port = 3000 }
in mine.port",
        "3000",
        "Integer",
    );
}

#[test]
fn test_record_spread_multiple_overrides() {
    assert_eval(
        "let r = {a = 1, b = 2, c = 3} in {...r, a = 10, c = 30}",
        "{a = 10, b = 2, c = 30}",
        "Rec {a : Integer, b : Integer, c : Integer}",
    );
}

// === Pipeline operator ===

#[test]
fn test_pipe_simple() {
    assert_eval("5 |> (\\x => x)", "5", "Integer");
}

#[test]
fn test_pipe_chain() {
    assert_eval(
        "let\n  f : Integer -> Integer\n  f x = x\nin 1 |> f |> f",
        "1",
        "Integer",
    );
}

// === File imports ===

#[test]
fn test_import_file() {
    let phox = phox::Phox::new();
    let result = phox
        .eval_file(std::path::Path::new("tests/fixtures/import_test.px"))
        .unwrap();
    assert_eq!(format!("{}", phox::TermPrinter(&result.term)), "21");
}

#[test]
fn test_import_stdlib() {
    let phox = phox::Phox::new();
    let result = phox
        .eval_file(std::path::Path::new("tests/fixtures/stdlib_test.px"))
        .unwrap();
    assert_eq!(format!("{}", phox::TermPrinter(&result.term)), "42");
}

#[test]
fn test_import_cyclic() {
    let phox = phox::Phox::new();
    let result = phox.eval_file(std::path::Path::new("tests/fixtures/cycle_a.px"));
    assert!(result.is_err());
}
