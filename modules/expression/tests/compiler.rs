use flaggo_contract::digests::{self, ExpressionCanonicalizer};
use flaggo_contract::validation::{self, ContractExpressionValidator, ExpressionPurpose};
use flaggo_contract::{DecisionContract, JsonValue, ValueSchema, strict_json};
use flaggo_expression::{EvaluationError, ExpressionCompiler, MAXIMUM_EXPRESSION_BYTES};
use indexmap::IndexMap;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn schema(kind: &str) -> ValueSchema {
    serde_json::from_value(json!({"type":kind})).unwrap()
}

fn attributes() -> IndexMap<String, ValueSchema> {
    IndexMap::from([
        ("level".into(), schema("integer")),
        ("label".into(), schema("string")),
    ])
}

#[test]
fn conditional_container_metadata_preserves_first_branch_parameters() {
    use base64::Engine;
    use cel_core_proto::r#gen::cel::expr::r#type::TypeKind;
    use prost::Message;

    let compiler = ExpressionCompiler::default();
    let types = [
        json!({"type":"array","items":{"type":"integer"}}),
        json!({"type":"array"}),
        json!({"type":"array","items":{"type":"array","items":{"type":"integer"}}}),
        json!({"type":"array","items":{"type":"array"}}),
    ];
    for (left_index, left) in types.iter().enumerate() {
        for (right_index, right) in types.iter().enumerate() {
            let attributes = IndexMap::from([
                ("a".into(), serde_json::from_value(left.clone()).unwrap()),
                ("b".into(), serde_json::from_value(right.clone()).unwrap()),
            ]);
            let compiled = compiler.compile(
                "true ? attributes.a : attributes.b",
                &attributes,
                &schema("array"),
                ExpressionPurpose::Result,
            );
            let compatible =
                !(left_index == 0 && right_index >= 2 || left_index >= 2 && right_index == 0);
            assert_eq!(compiled.is_ok(), compatible, "{left_index}/{right_index}");
            let Ok(compiled) = compiled else { continue };
            compiler
                .materialize(
                    compiled.checked(),
                    &attributes,
                    &schema("array"),
                    ExpressionPurpose::Result,
                )
                .unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&compiled.checked().checked_ast)
                .unwrap();
            let wire = cel_core_proto::CheckedExpr::decode(bytes.as_slice()).unwrap();
            let mut kind = wire.type_map.get(&wire.expr.as_ref().unwrap().id).unwrap();
            for _ in 0..(if left_index >= 2 { 2 } else { 1 }) {
                let Some(TypeKind::ListType(list)) = &kind.type_kind else {
                    panic!("expected a list type for {left_index}/{right_index}");
                };
                kind = list.elem_type.as_deref().unwrap();
            }
            if left_index % 2 == 0 {
                assert!(matches!(kind.type_kind, Some(TypeKind::Primitive(2))));
            } else {
                assert!(matches!(kind.type_kind, Some(TypeKind::Dyn(_))));
            }
        }
    }
}

#[test]
fn conditional_generic_substitution_keeps_reference_branch_order() {
    let compiler = ExpressionCompiler::default();
    assert!(
        compiler
            .compile(
                "false ? (null + null) : 1",
                &attributes(),
                &schema("integer"),
                ExpressionPurpose::Result,
            )
            .is_err()
    );
    for (source, kind, expected) in [
        ("true ? null : (null + null)", "null", JsonValue::Null),
        (
            "true ? 1 : (null + null)",
            "integer",
            JsonValue::from(json!(1)),
        ),
        (
            "false && bool(true ? (null + null) : attributes.items)",
            "boolean",
            JsonValue::Bool(false),
        ),
    ] {
        let mut attributes = attributes();
        attributes.insert(
            "items".to_owned(),
            serde_json::from_value(json!({"type":"array","items":{"type":"integer"}})).unwrap(),
        );
        let compiled = compiler
            .compile(
                source,
                &attributes,
                &schema(kind),
                ExpressionPurpose::Result,
            )
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        let restarted = compiler
            .materialize(
                compiled.checked(),
                &attributes,
                &schema(kind),
                ExpressionPurpose::Result,
            )
            .unwrap();
        for expression in [compiled, restarted] {
            assert_eq!(
                expression.evaluate(&IndexMap::new(), None).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn guarded_nullable_overloads_compile_materialize_and_keep_root_types_strict() {
    let compiler = ExpressionCompiler::default();
    for (source, expected) in [
        ("false && null < null", false),
        ("true || null < null", true),
        ("false && (null + null == null + null)", false),
        ("false && (int(null + null) == 0)", false),
        ("false && bool(null - null)", false),
        ("false && (size(null + null) == 0)", false),
        ("false && bool((null + null) * (null - null))", false),
    ] {
        let compiled = compiler
            .compile(
                source,
                &IndexMap::new(),
                &schema("boolean"),
                ExpressionPurpose::Predicate,
            )
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        let restarted = compiler
            .materialize(
                compiled.checked(),
                &IndexMap::new(),
                &schema("boolean"),
                ExpressionPurpose::Predicate,
            )
            .unwrap();
        for expression in [compiled, restarted] {
            assert_eq!(
                expression.evaluate(&IndexMap::new(), None).unwrap(),
                JsonValue::Bool(expected)
            );
        }
    }
    for kind in [
        "null", "boolean", "integer", "number", "string", "array", "object",
    ] {
        assert!(
            compiler
                .compile(
                    "null + null",
                    &IndexMap::new(),
                    &schema(kind),
                    ExpressionPurpose::Result,
                )
                .is_err(),
            "{kind}"
        );
    }
}

#[test]
fn conditional_overloads_unify_nested_container_parameters() {
    let compiler = ExpressionCompiler::default();
    for (left, right, accepted) in [
        (
            json!({"type":"array","items":{"type":"integer"}}),
            json!({"type":"array","items":{"type":"string"}}),
            false,
        ),
        (
            json!({"type":"array","items":{"type":"array","items":{"type":"integer"}}}),
            json!({"type":"array","items":{"type":"array","items":{"type":"string"}}}),
            false,
        ),
        (
            json!({"type":"array","items":{"type":"integer"}}),
            json!({"type":"array","items":{"type":"integer"}}),
            true,
        ),
        (
            json!({"type":"array"}),
            json!({"type":"array","items":{"type":"integer"}}),
            true,
        ),
    ] {
        let attributes = IndexMap::from([
            ("choose".into(), schema("boolean")),
            ("a".into(), serde_json::from_value(left).unwrap()),
            ("b".into(), serde_json::from_value(right).unwrap()),
        ]);
        let result = compiler.compile(
            "size(attributes.choose ? attributes.a : attributes.b)",
            &attributes,
            &schema("integer"),
            ExpressionPurpose::Result,
        );
        assert_eq!(result.is_ok(), accepted);
        if let Ok(compiled) = result {
            compiler
                .materialize(
                    compiled.checked(),
                    &attributes,
                    &schema("integer"),
                    ExpressionPurpose::Result,
                )
                .unwrap();
        }
    }
}

#[test]
fn overflowing_literals_keep_oracle_canonical_text_and_restart() {
    let compiler = ExpressionCompiler::default();
    let compiled = compiler
        .compile(
            "1e400 > 0.0",
            &IndexMap::new(),
            &schema("boolean"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    assert_eq!(compiled.checked().canonical_expression, "+Inf > 0.0");
    let restarted = compiler
        .materialize(
            compiled.checked(),
            &IndexMap::new(),
            &schema("boolean"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    assert_eq!(
        restarted.evaluate(&IndexMap::new(), None).unwrap(),
        JsonValue::from(json!(true))
    );
}

fn contract() -> DecisionContract {
    strict_json::deserialize(
        br#"{
        "authority":{"tenant":"local","application":"parity","environment":"test"},
        "name":"compiler.parity","expression_syntax":"flaggo.cel/v1",
        "attributes":[{"name":"level","schema":{"type":"integer"}}],
        "result":{"schema":{"type":"number"},"default":0},
        "authoredExecutable":{"rules":[{
            "name":"r","when":{"expression":" attributes.level>= 1 "},
            "return":{"expression":"double( attributes.level )+ 2.0"}
        }]}
    }"#,
    )
    .unwrap()
}

#[test]
fn compilation_owns_checked_metadata_and_materialization_needs_no_source() {
    let compiler = ExpressionCompiler::default();
    let source = " double( attributes.level )+ 2.5 ";
    let compiled = compiler
        .compile(
            source,
            &attributes(),
            &schema("number"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    assert_eq!(compiled.source(), source);
    assert_eq!(compiled.purpose(), ExpressionPurpose::Result);
    assert_eq!(
        compiled.checked().canonical_expression,
        "double(attributes.level) + 2.5"
    );
    assert_eq!(compiled.checked().referenced_attributes, ["level"]);
    let stored = serde_json::to_string(compiled.checked()).unwrap();
    let reloaded = compiler
        .materialize(
            &serde_json::from_str(&stored).unwrap(),
            &attributes(),
            &schema("number"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    assert_eq!(reloaded.source(), compiled.checked().canonical_expression);
    let input = IndexMap::from([("level".into(), JsonValue::from(json!(3)))]);
    assert_eq!(
        compiled.evaluate(&input, None).unwrap(),
        JsonValue::from(json!(5.5))
    );
    assert_eq!(
        compiled.evaluate(&input, None).unwrap(),
        reloaded.evaluate(&input, None).unwrap()
    );
}

#[test]
fn predicates_ignore_the_result_schema_and_number_results_accept_integers() {
    let compiler = ExpressionCompiler::default();
    let predicate = compiler
        .compile(
            "attributes.level > 1",
            &attributes(),
            &schema("string"),
            ExpressionPurpose::Predicate,
        )
        .unwrap();
    assert_eq!(predicate.purpose(), ExpressionPurpose::Predicate);
    assert!(
        compiler
            .compile(
                "1",
                &attributes(),
                &schema("number"),
                ExpressionPurpose::Result,
            )
            .is_ok()
    );
    assert!(
        compiler
            .compile(
                "1.0",
                &attributes(),
                &schema("integer"),
                ExpressionPurpose::Result,
            )
            .is_err()
    );
    assert!(
        compiler
            .compile(
                "1",
                &attributes(),
                &schema("number"),
                ExpressionPurpose::Predicate,
            )
            .is_err()
    );
}

#[test]
fn type_checking_does_not_serialize_unrelated_numeric_schema_bounds() {
    let compiler = ExpressionCompiler::default();
    let mut attributes = attributes();
    attributes.get_mut("level").unwrap().minimum = Some(f64::INFINITY);
    let mut result = schema("number");
    result.maximum = Some(f64::NEG_INFINITY);
    let compiled = compiler
        .compile(
            "attributes.level",
            &attributes,
            &result,
            ExpressionPurpose::Result,
        )
        .unwrap();
    assert!(
        compiler
            .materialize(
                compiled.checked(),
                &attributes,
                &result,
                ExpressionPurpose::Result
            )
            .is_ok()
    );
}

#[test]
fn production_adapter_validates_and_normalizes_contract_identities() {
    let compiler = ExpressionCompiler::default();
    let contract = contract();
    let validation = validation::validate_contract(&contract, Some(&compiler)).unwrap();
    assert_eq!(validation.status, "valid");
    let normalized = digests::canonicalize_expressions(&contract, &compiler).unwrap();
    assert_eq!(
        validation.contract_digest.unwrap(),
        digests::contract_digest(&normalized, Some(&compiler)).unwrap()
    );
    assert_ne!(
        digests::contract_digest(&contract, None).unwrap(),
        digests::contract_digest(&contract, Some(&compiler)).unwrap()
    );
    assert_eq!(
        digests::canonicalize_expressions(&normalized, &compiler).unwrap(),
        normalized
    );
}

#[test]
fn expression_validator_returns_references_and_rejects_undeclared_attributes() {
    let compiler = ExpressionCompiler::default();
    let valid = compiler.validate(
        "attributes.level > 0",
        &attributes(),
        &schema("boolean"),
        ExpressionPurpose::Predicate,
    );
    assert!(valid.is_valid);
    assert_eq!(
        valid.referenced_attributes.into_iter().collect::<Vec<_>>(),
        ["level"]
    );
    let invalid = compiler.validate(
        "attributes.missing",
        &attributes(),
        &schema("integer"),
        ExpressionPurpose::Result,
    );
    assert!(!invalid.is_valid);
    assert!(invalid.canonical_expression.is_none());
    assert!(invalid.referenced_attributes.is_empty());
    assert!(invalid.error.unwrap().contains("undeclared attribute"));
}

#[test]
fn syntax_only_validation_does_not_type_check_guardrail_declarations() {
    let compiler = ExpressionCompiler::default();
    let syntax = compiler.validate_syntax("sum( evidence.recovery_failure )<= 0");
    assert!(syntax.is_valid);
    assert_eq!(
        syntax.canonical_expression.as_deref(),
        Some("sum(evidence.recovery_failure) <= 0")
    );
    assert!(syntax.referenced_attributes.is_empty());
    assert!(!compiler.validate_syntax("not valid {").is_valid);
    assert_eq!(
        compiler.canonicalize("{'a': [1, 2]}['a']").unwrap(),
        "{\"a\": [1, 2]}[\"a\"]"
    );
    assert_eq!(compiler.canonicalize("b'abc'").unwrap(), "b\"abc\"");
    assert_eq!(
        compiler.canonicalize("b'\\t\\n\\r'").unwrap(),
        "b\"\\x09\\x0a\\x0d\""
    );
    assert_eq!(
        compiler.canonicalize("b'\\x22\\x5c'").unwrap(),
        "b\"\\x22\\x5c\""
    );
}

#[test]
fn syntax_keeps_macros_as_ordinary_calls_but_the_executable_profile_rejects_them() {
    let compiler = ExpressionCompiler::default();
    for (source, canonical, function) in [
        ("has( 1 )", "has(1)", "has"),
        ("[1,2].map(x,x+1)", "[1, 2].map(x, x + 1)", "map"),
        (
            "evidence.items.filter(x,x>0)",
            "evidence.items.filter(x, x > 0)",
            "filter",
        ),
    ] {
        assert_eq!(compiler.canonicalize(source).unwrap(), canonical);
        let error = compiler
            .compile(
                source,
                &attributes(),
                &schema("boolean"),
                ExpressionPurpose::Predicate,
            )
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            format!("Function or operator '{function}' is not supported by flaggo.cel/v1.")
        );
    }
}

#[test]
fn grammar_preserves_reference_collections_unary_folding_and_conditional_associativity() {
    let compiler = ExpressionCompiler::default();
    for (source, expected) in [
        ("[,]", "[]"),
        ("{,}", "{}"),
        ("Type{,}", "Type{}"),
        ("[1,]", "[1]"),
        ("{1:2,}", "{1: 2}"),
        ("--1", "1"),
        ("!!!true", "!true"),
        ("-0x8000000000000000", "-9223372036854775808"),
        ("a?b:c?d:e", "a ? b : c ? d : e"),
        ("a?(b?c:d):e", "a ? (b ? c : d) : e"),
    ] {
        assert_eq!(compiler.canonicalize(source).unwrap(), expected, "{source}");
    }
    for source in ["f(1,)", "a?b?c:d:e", "!-1", "-!true", "[1,,2]", "Type{1:2}"] {
        assert!(compiler.canonicalize(source).is_err(), "{source}");
    }
}

#[test]
fn native_parser_and_lexer_diagnostics_include_exact_utf16_locations() {
    let compiler = ExpressionCompiler::default();
    for (source, expected) in [
        (
            "not valid {",
            "ERROR: 1:5: unexpected token 'valid' after expression",
        ),
        ("a.?b", "ERROR: 1:3: unsupported syntax '.?'"),
        ("a[?1]", "ERROR: 1:3: unsupported syntax '[?'"),
        ("[?1]", "ERROR: 1:2: unsupported syntax '?'"),
        ("int(1,)", "ERROR: 1:7: unexpected token ')'"),
        (
            "9223372036854775808",
            "ERROR: 1:1: invalid int literal: value out of range",
        ),
        (
            "18446744073709551616u",
            "ERROR: 1:1: invalid unsigned integer literal: value out of range",
        ),
        ("not valid { @", "ERROR: 1:13: unexpected character '@'"),
        ("'😀\\q'", "ERROR: 1:5: invalid escape sequence '\\q'"),
        (
            "true &&\n'\\x0'",
            "ERROR: 2:4: invalid hex escape: too few digits",
        ),
    ] {
        assert_eq!(
            compiler.canonicalize(source).unwrap_err(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn lexical_decoding_preserves_raw_unicode_and_octet_semantics() {
    let compiler = ExpressionCompiler::default();
    for (source, expected) in [
        (r"r'\q'", r#""\\q""#),
        (r"br'\x41'", r#"b"\x5cx41""#),
        (r"b'\303\251'", r#"b"\xc3\xa9""#),
        (r"'\u00e9\U0001f600'", "\"é😀\""),
        ("'''first\nsecond'''", r#""first\nsecond""#),
    ] {
        assert_eq!(compiler.canonicalize(source).unwrap(), expected, "{source}");
    }
    for (source, expected) in [
        (
            r"'\uD800'",
            "ERROR: 1:3: invalid unicode escape: surrogate code points are not allowed",
        ),
        (
            r"'\U00110000'",
            "ERROR: 1:3: invalid unicode escape: code point out of range",
        ),
        (r"b'\u0041'", "ERROR: 1:4: invalid escape sequence '\\u'"),
        (
            "attributes.`😀`",
            "ERROR: 1:13: invalid character '\u{fffd}' in escaped identifier",
        ),
    ] {
        assert_eq!(
            compiler.canonicalize(source).unwrap_err(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn diagnostic_type_names_preserve_container_parameters() {
    let compiler = ExpressionCompiler::default();
    let attributes = IndexMap::from([
        (
            "values".into(),
            serde_json::from_value(json!({"type": "array", "items": {"type": "integer"}})).unwrap(),
        ),
        ("object".into(), schema("object")),
    ]);
    for (source, expected) in [
        (
            "attributes.values",
            "Expression has type 'list(int)', expected 'bool'.",
        ),
        (
            "attributes.values + 1",
            "ERROR: 1:19: found no matching overload for '_+_' applied to '(list(int), int)'",
        ),
        (
            "attributes.object + 1",
            "ERROR: 1:19: found no matching overload for '_+_' applied to '(map(string, dyn), int)'",
        ),
    ] {
        let error = compiler
            .compile(
                source,
                &attributes,
                &schema("boolean"),
                ExpressionPurpose::Predicate,
            )
            .err()
            .unwrap();
        assert_eq!(error.to_string(), expected, "{source}");
    }
    let expected =
        serde_json::from_value(json!({"type": "array", "items": {"type": "integer"}})).unwrap();
    let error = compiler
        .compile("true", &attributes, &expected, ExpressionPurpose::Result)
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "Expression has type 'bool', expected 'list(int)'."
    );
}

#[test]
fn typing_collects_sibling_errors_without_cascading_and_follows_profile_then_declarations() {
    let compiler = ExpressionCompiler::default();
    for (source, expected) in [
        (
            "unknown(attributes.missing)",
            "Function or operator 'unknown' is not supported by flaggo.cel/v1.",
        ),
        (
            "(1 + \"x\") == [1]",
            "list literals are not supported by flaggo.cel/v1.",
        ),
        (
            "(1 + \"x\") == attributes.missing",
            "Expression references undeclared attribute 'missing'.",
        ),
        (
            "(1 + \"x\") == (true + 2)",
            "ERROR: 1:4: found no matching overload for '_+_' applied to '(int, string)'; ERROR: 1:20: found no matching overload for '_+_' applied to '(bool, int)'",
        ),
        (
            "\"😀\" + 1 == 0",
            "ERROR: 1:6: found no matching overload for '_+_' applied to '(string, int)'",
        ),
        (
            "true &&\n(\"😀\" + 1 == 2)",
            "ERROR: 2:7: found no matching overload for '_+_' applied to '(string, int)'",
        ),
    ] {
        let error = compiler
            .compile(
                source,
                &attributes(),
                &schema("boolean"),
                ExpressionPurpose::Predicate,
            )
            .err()
            .unwrap();
        assert_eq!(error.to_string(), expected, "{source}");
    }
}

#[test]
fn syntax_nesting_and_flat_ast_limits_do_not_depend_on_the_native_stack() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let compiler = ExpressionCompiler::default();
            for (open, close) in [
                ("(", ")"),
                ("[", "]"),
                ("f(", ")"),
                ("{1:", "}"),
                ("Type{field:", "}"),
            ] {
                let source = format!("{}1{}", open.repeat(249), close.repeat(249));
                assert!(compiler.canonicalize(&source).is_ok(), "{open}");
                let source = format!("{}1{}", open.repeat(250), close.repeat(250));
                assert!(
                    compiler
                        .canonicalize(&source)
                        .unwrap_err()
                        .ends_with("expression recursion limit exceeded: 250"),
                    "{open}"
                );
            }
            assert!(compiler.canonicalize(&vec!["1"; 1000].join("+")).is_ok());
            assert_eq!(
                compiler
                    .canonicalize(&vec!["1"; 1001].join("+"))
                    .unwrap_err(),
                "ERROR: 1:1: expression depth limit exceeded: 1001 > 1000"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn static_cost_diagnostics_retain_ranges_and_follow_result_type_validation() {
    let compiler = ExpressionCompiler::default();
    let source = format!("\"{}\" == \"{}\"", "a".repeat(1001), "a".repeat(1001));
    let error = compiler
        .compile(
            &source,
            &attributes(),
            &schema("boolean"),
            ExpressionPurpose::Result,
        )
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "Expression static cost [1, 1001] exceeds 1000."
    );
    let error = compiler
        .compile(
            "string(1) + 'x'",
            &attributes(),
            &schema("boolean"),
            ExpressionPurpose::Result,
        )
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "Expression has type 'string', expected 'bool'."
    );
    let error = compiler
        .compile(
            "string(1) == '1'",
            &attributes(),
            &schema("boolean"),
            ExpressionPurpose::Result,
        )
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "Expression static cost [2, unbounded] exceeds 1000."
    );
}

#[test]
fn source_byte_bounds_are_exact_and_runtime_profile_is_not_syntax_only() {
    let compiler = ExpressionCompiler::default();
    let source = format!("{}1", " ".repeat(MAXIMUM_EXPRESSION_BYTES - 1));
    assert!(
        compiler
            .compile(
                &source,
                &attributes(),
                &schema("integer"),
                ExpressionPurpose::Result
            )
            .is_ok()
    );
    assert!(
        compiler
            .compile(
                &format!(" {source}"),
                &attributes(),
                &schema("integer"),
                ExpressionPurpose::Result,
            )
            .is_err()
    );
    for source in [
        "[1, 2]",
        "attributes.level + attributes.label",
        "unknown(1)",
    ] {
        assert!(
            compiler
                .compile(
                    source,
                    &attributes(),
                    &schema("integer"),
                    ExpressionPurpose::Result
                )
                .is_err()
        );
    }
    assert!(compiler.canonicalize("[1, 2]").is_ok());
}

#[test]
fn materialization_rejects_tampered_metadata_and_undeclared_references() {
    let compiler = ExpressionCompiler::default();
    let compiled = compiler
        .compile(
            "attributes.level + 1",
            &attributes(),
            &schema("integer"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    for mutation in ["digest", "canonical", "references", "duplicate"] {
        let mut stored = compiled.checked().clone();
        match mutation {
            "digest" => stored.checked_ast_digest = format!("sha256:{}", "0".repeat(64)),
            "canonical" => stored.canonical_expression = "0".into(),
            "references" => stored.referenced_attributes.clear(),
            "duplicate" => stored.referenced_attributes.push("level".into()),
            _ => unreachable!(),
        }
        assert!(
            compiler
                .materialize(
                    &stored,
                    &attributes(),
                    &schema("integer"),
                    ExpressionPurpose::Result,
                )
                .is_err(),
            "{mutation}"
        );
    }
    assert!(
        compiler
            .materialize(
                compiled.checked(),
                &IndexMap::new(),
                &schema("integer"),
                ExpressionPurpose::Result,
            )
            .is_err()
    );
}

#[test]
fn loop_free_programs_preserve_reference_cancellation_behavior() {
    let compiler = ExpressionCompiler::default();
    let compiled = compiler
        .compile(
            "true",
            &attributes(),
            &schema("boolean"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    let reloaded = compiler
        .materialize(
            compiled.checked(),
            &attributes(),
            &schema("boolean"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    for expression in [compiled, reloaded] {
        assert_eq!(
            expression
                .evaluate(&IndexMap::new(), Some(Arc::new(AtomicBool::new(true))))
                .unwrap(),
            JsonValue::Bool(true)
        );
        assert_eq!(
            expression
                .evaluate(&IndexMap::new(), Some(Arc::new(AtomicBool::new(false))),)
                .unwrap(),
            JsonValue::Bool(true)
        );
    }
}

#[test]
fn boolean_short_circuit_and_error_absorption_are_preserved() {
    let compiler = ExpressionCompiler::default();
    for (source, expected) in [
        ("false && (1 / 0 == 1)", false),
        ("(1 / 0 == 1) && false", false),
        ("true || (1 / 0 == 1)", true),
        ("(1 / 0 == 1) || true", true),
    ] {
        let expression = compiler
            .compile(
                source,
                &attributes(),
                &schema("boolean"),
                ExpressionPurpose::Result,
            )
            .unwrap();
        assert_eq!(
            expression.evaluate(&IndexMap::new(), None).unwrap(),
            JsonValue::Bool(expected)
        );
    }
}

#[test]
fn evaluation_faults_remain_distinct_from_non_finite_serialization_faults() {
    let compiler = ExpressionCompiler::default();
    let divide = compiler
        .compile(
            "1 / 0",
            &attributes(),
            &schema("integer"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    assert!(matches!(
        divide.evaluate(&IndexMap::new(), None),
        Err(EvaluationError::Failed(_))
    ));
    let non_finite = compiler
        .compile(
            "double('NaN')",
            &attributes(),
            &schema("number"),
            ExpressionPurpose::Result,
        )
        .unwrap();
    assert!(matches!(
        non_finite.evaluate(&IndexMap::new(), None),
        Err(EvaluationError::NonFiniteResult)
    ));
}
