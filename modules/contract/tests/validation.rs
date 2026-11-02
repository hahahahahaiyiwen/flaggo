use flaggo_contract::digests::ExpressionCanonicalizer;
use flaggo_contract::strict_json;
use flaggo_contract::validation::{
    self, ContractExpressionValidator, ExpressionPurpose, ExpressionValidation,
};
use flaggo_contract::value_schema::{validate_schema, validate_value};
use flaggo_contract::{DecisionContract, JsonValue, RuntimeInput, ValueSchema};
use indexmap::IndexMap;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashSet;

fn fixture() -> DecisionContract {
    #[derive(serde::Deserialize)]
    struct Fixture {
        request: Request,
    }
    #[derive(serde::Deserialize)]
    struct Request {
        body: Box<serde_json::value::RawValue>,
    }
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../contracts/fixtures/management/decision-contract/01-validate-valid.json"
    ))
    .unwrap();
    strict_json::deserialize(fixture.request.body.get().as_bytes()).unwrap()
}

fn schema(value: Value) -> ValueSchema {
    serde_json::from_value(value).unwrap()
}

#[test]
fn shared_fixture_validates_with_its_exact_published_identity() {
    let result = validation::validate_contract(&fixture(), None).unwrap();
    assert_eq!(result.status, "valid");
    assert!(result.issues.is_empty());
    assert_eq!(
        result.contract_digest.as_deref(),
        Some("sha256:7394e6ec3d9eee8d559c4e5b44b15fb5ab7827e5307d5877f369ba0c9ded8098")
    );
}

#[test]
fn numeric_bound_precedence_and_inapplicable_keywords_preserve_legacy_rules() {
    let mut issues = Vec::new();
    let number = schema(json!({"type":"number","minimum":0,"exclusiveMinimum":10,"maximum":5}));
    validate_schema(&number, "/schema", &mut issues);
    assert!(issues.is_empty());
    validate_value(&number, &json!(1).into(), "/value", &mut issues).unwrap();
    assert_eq!(issues[0].code, "number-out-of-range");
    issues.clear();
    let array = schema(json!({"type":"array","items":{"type":"integer"},"minLength":4}));
    validate_schema(&array, "/schema", &mut issues);
    assert!(issues.is_empty());
    validate_schema(
        &schema(json!({"type":"boolean","minLength":4})),
        "/schema",
        &mut issues,
    );
    assert_eq!(issues[0].code, "inapplicable-schema-keyword");
}

#[test]
fn integer_validation_requires_lexical_int64_not_an_integral_float() {
    let schema = schema(json!({"type":"integer"}));
    for (text, valid) in [
        ("1", true),
        ("0", true),
        ("-0", true),
        (" \t-0\r\n", true),
        ("9223372036854775807", true),
        ("-9223372036854775808", true),
        ("9223372036854775808", false),
        ("1.0", false),
        ("1e0", false),
        ("-0.0", false),
        ("-0e0", false),
        ("-0E+0", false),
    ] {
        let mut issues = Vec::new();
        let value: JsonValue = serde_json::from_str(text).unwrap();
        validate_value(&schema, &value, "/value", &mut issues).unwrap();
        assert_eq!(issues.is_empty(), valid, "{text}");
    }
}

#[test]
fn string_bounds_count_unicode_scalars_and_not_utf16_units_or_utf8_bytes() {
    let schema = schema(json!({"type":"string","minLength":1,"maxLength":1}));
    for (text, valid) in [("\u{1f600}", true), ("e\u{301}", false)] {
        let mut issues = Vec::new();
        validate_value(&schema, &json!(text).into(), "/value", &mut issues).unwrap();
        assert_eq!(issues.is_empty(), valid);
    }
}

#[test]
fn uniqueness_uses_canonical_json_including_ieee754_integer_rounding() {
    let schema = schema(json!({"type":"array","uniqueItems":true}));
    for values in [
        json!([1, 1.0]),
        json!([9007199254740992_i64, 9007199254740993_i64]),
        json!([{"a":1,"b":2},{"b":2,"a":1}]),
    ] {
        let mut issues = Vec::new();
        validate_value(&schema, &values.into(), "/value", &mut issues).unwrap();
        assert_eq!(issues[0].code, "array-items-not-unique");
    }
}

#[test]
fn literal_property_order_is_retained_for_validation_issues_and_pointer_escaping() {
    let schema = schema(json!({"type":"object","properties":{}}));
    let value: JsonValue = strict_json::deserialize(br#"{"z/~":1,"a":2}"#).unwrap();
    let mut issues = Vec::new();
    validate_value(&schema, &value, "/value", &mut issues).unwrap();
    assert_eq!(
        issues
            .iter()
            .map(|issue| issue.path.as_str())
            .collect::<Vec<_>>(),
        ["/value/z~1~0", "/value/a"]
    );
    let bytes = serde_json::to_string(&value).unwrap();
    assert_eq!(bytes, r#"{"z/~":1,"a":2}"#);
}

#[test]
fn schema_and_value_depth_limits_count_from_zero_and_reject_only_above_sixteen() {
    for depth in [16, 17] {
        let mut schema = schema(json!({"type":"integer"}));
        let mut value: JsonValue = json!(1).into();
        for _ in 0..depth {
            let mut outer = serde_json::from_value::<ValueSchema>(json!({"type":"array"})).unwrap();
            outer.items = Some(Box::new(schema));
            schema = outer;
            value = JsonValue::Array(vec![value]);
        }
        let mut issues = Vec::new();
        validate_schema(&schema, "/schema", &mut issues);
        assert_eq!(issues.is_empty(), depth == 16);
        if depth == 17 {
            assert_eq!(issues[0].code, "schema-too-deep");
        }
        issues.clear();
        validate_value(&schema, &value, "/value", &mut issues).unwrap();
        assert_eq!(issues.is_empty(), depth == 16);
        if depth == 17 {
            assert_eq!(issues[0].code, "value-too-deep");
        }
    }
}

#[test]
fn runtime_input_requires_random_but_does_not_require_every_declared_attribute() {
    let contract = fixture();
    let input: RuntimeInput = strict_json::deserialize(br#"{"attributes":{"_random":0}}"#).unwrap();
    let mut issues = Vec::new();
    validation::validate_runtime_input(&contract, &input, &mut issues).unwrap();
    assert!(issues.is_empty());
    let input =
        strict_json::deserialize(br#"{"attributes":{"z/~":1,"_internal":0,"a":2}}"#).unwrap();
    validation::validate_runtime_input(&contract, &input, &mut issues).unwrap();
    assert_eq!(
        issues
            .iter()
            .map(|issue| issue.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/attributes/_random",
            "/attributes/z~1~0",
            "/attributes/_internal",
            "/attributes/a"
        ]
    );
}

#[test]
fn duration_digit_and_final_newline_behavior_matches_utf16_reference_regex() {
    for (interval, valid) in [
        ("P1D\n", true),
        ("P1D\r\n", false),
        ("P\u{661}Y1M", true),
        ("P\u{661}D", false),
        ("P\u{1d7d9}Y1M", false),
        ("P1YT", false),
    ] {
        let mut contract = fixture();
        contract.learning.as_mut().unwrap().policy.evaluate.interval = interval.into();
        let result = validation::validate_contract(&contract, None).unwrap();
        assert_eq!(result.status == "valid", valid, "{interval:?}");
    }
    assert!(validation::is_decision_name("accepted\n"));
    assert!(!validation::is_decision_name("rejected\r\n"));
}

#[derive(Default)]
struct Validator {
    calls: RefCell<Vec<String>>,
    reject_predicates: bool,
}

impl ExpressionCanonicalizer for Validator {
    fn canonicalize(&self, expression: &str) -> Result<String, String> {
        self.calls.borrow_mut().push("canonicalize".into());
        Ok(expression.trim().into())
    }
}

impl ContractExpressionValidator for Validator {
    fn validate(
        &self,
        _: &str,
        _: &IndexMap<String, ValueSchema>,
        _: &ValueSchema,
        purpose: ExpressionPurpose,
    ) -> ExpressionValidation {
        self.calls.borrow_mut().push(format!("{purpose:?}"));
        let rejected = self.reject_predicates && purpose == ExpressionPurpose::Predicate;
        ExpressionValidation {
            is_valid: !rejected,
            canonical_expression: None,
            referenced_attributes: HashSet::new(),
            error: rejected.then(|| "test rejection".into()),
        }
    }
    fn validate_syntax(&self, _: &str) -> ExpressionValidation {
        self.calls.borrow_mut().push("syntax".into());
        ExpressionValidation {
            is_valid: true,
            canonical_expression: None,
            referenced_attributes: HashSet::new(),
            error: None,
        }
    }
}

#[test]
fn expression_port_keeps_predicate_result_and_guardrail_validation_separate() {
    let validator = Validator::default();
    let result = validation::validate_contract(&fixture(), Some(&validator)).unwrap();
    assert_eq!(result.status, "valid");
    assert_eq!(
        *validator.calls.borrow(),
        [
            "Predicate",
            "Predicate",
            "Result",
            "syntax",
            "canonicalize",
            "canonicalize",
            "canonicalize",
            "canonicalize",
        ]
    );
    let validator = Validator {
        reject_predicates: true,
        ..Validator::default()
    };
    let result = validation::validate_contract(&fixture(), Some(&validator)).unwrap();
    assert_eq!(result.status, "invalid");
    assert_eq!(result.issues.len(), 2);
    assert!(
        result
            .issues
            .iter()
            .all(|issue| issue.code == "invalid-expression" && issue.message == "test rejection")
    );
    assert!(
        !validator
            .calls
            .borrow()
            .iter()
            .any(|call| call == "canonicalize")
    );
}
