//! Shared canonical identity and numeric-boundary corpus for pinned consumers.
use flaggo_contract::{JsonValue, ValueSchema, digests, strict_json, value_schema};
use flaggo_expression::ExpressionCompiler;
use serde_json::json;

pub const FIXTURE_REVISION: &str = "flaggo.canonical-conformance/v1";

pub fn c01_canonical() {
    let compiler = ExpressionCompiler::default();
    let original = crate::contract();
    let digest = digests::contract_digest(&original, Some(&compiler)).unwrap();
    assert_eq!(
        digest,
        "sha256:7394e6ec3d9eee8d559c4e5b44b15fb5ab7827e5307d5877f369ba0c9ded8098"
    );
    let mut reordered = original.clone();
    reordered.attributes.reverse();
    reordered.result.schema.description = Some("presentation only".into());
    reordered.attributes[0].as_mut().unwrap().schema.description = Some("presentation only".into());
    assert_eq!(
        digest,
        digests::contract_digest(&reordered, Some(&compiler)).unwrap()
    );
    let mut collections = original.clone();
    let learning = collections.learning.as_mut().unwrap();
    let guardrails = learning.objective.guardrails.as_mut().unwrap();
    let mut extra = guardrails[0].clone().unwrap();
    extra.name = "second-guardrail".into();
    guardrails.push(Some(extra));
    let collection_digest = digests::contract_digest(&collections, Some(&compiler)).unwrap();
    let learning = collections.learning.as_mut().unwrap();
    assert!(learning.evidence.len() > 1);
    learning.evidence.reverse();
    learning.objective.guardrails.as_mut().unwrap().reverse();
    learning.evidence[0].as_mut().unwrap().description = Some("presentation only".into());
    assert_eq!(
        collection_digest,
        digests::contract_digest(&collections, Some(&compiler)).unwrap()
    );

    let mut literals = original.clone();
    literals.authored_executable = None;
    literals.result.schema = serde_json::from_value(json!({
        "type":"array", "items":{"type":"object","properties":{"description":{"type":"string"}}}
    }))
    .unwrap();
    literals.result.default = json!([{"description":"first"},{"description":"second"}]).into();
    let literal_digest = digests::contract_digest(&literals, Some(&compiler)).unwrap();
    literals.result.default = json!([{"description":"second"},{"description":"first"}]).into();
    assert_ne!(
        literal_digest,
        digests::contract_digest(&literals, Some(&compiler)).unwrap()
    );
    literals.result.default = json!([{"description":"changed"},{"description":"second"}]).into();
    assert_ne!(
        literal_digest,
        digests::contract_digest(&literals, Some(&compiler)).unwrap()
    );
    for component in 0..3 {
        let mut changed = original.clone();
        match component {
            0 => changed.authority.tenant = Some("another-tenant".into()),
            1 => changed.authority.application = Some("another-application".into()),
            _ => changed.authority.environment = Some("another-environment".into()),
        }
        assert_ne!(
            digest,
            digests::contract_digest(&changed, Some(&compiler)).unwrap()
        );
    }
    let mut changed = original.clone();
    changed
        .authored_executable
        .as_mut()
        .unwrap()
        .rules
        .reverse();
    assert_ne!(
        digest,
        digests::contract_digest(&changed, Some(&compiler)).unwrap()
    );
    let mut normalized = original.clone();
    normalized.authored_executable.as_mut().unwrap().rules[0]
        .as_mut()
        .unwrap()
        .when =
        flaggo_contract::RuleWhen::Expression("  attributes.board_pressure >= 0.8  ".into());
    assert_eq!(
        digest,
        digests::contract_digest(&normalized, Some(&compiler)).unwrap()
    );

    let integer: ValueSchema = serde_json::from_value(json!({"type":"integer"})).unwrap();
    for (token, valid) in [
        ("9223372036854775807", true),
        ("-9223372036854775808", true),
        ("9007199254740993", true),
        ("9223372036854775808", false),
        ("1.0", false),
        ("1e0", false),
        ("-0", true),
        ("-0.0", false),
        ("1e400", false),
    ] {
        let value: JsonValue = strict_json::deserialize(token.as_bytes()).unwrap();
        assert_eq!(strict_json::serialize(&value).unwrap(), token.as_bytes());
        let mut issues = Vec::new();
        value_schema::validate_value(&integer, &value, "/value", &mut issues).unwrap();
        assert_eq!(issues.is_empty(), valid, "{token}");
    }
    for malformed in [r#"{"a":1,"a":2}"#, r#"{"a":NaN}"#, r#"{"a":1}{}"#] {
        assert!(strict_json::deserialize::<JsonValue>(malformed.as_bytes()).is_err());
    }
    let executable = flaggo_contract::DecisionExecutable {
        contract_digest: digest,
        kind: "rules".into(),
        rules: serde_json::from_value(
            serde_json::to_value(&original.authored_executable.as_ref().unwrap().rules).unwrap(),
        )
        .unwrap(),
    };
    let compiled = flaggo_expression::ExecutableCompiler::default()
        .compile(&original, &executable)
        .unwrap();
    for (pressure, level, result, rule) in [
        (0.9, 10, 250, Some("high-pressure")),
        (0.2, 10, 750, Some("level-adjustment")),
        (0.2, 1, 500, None),
    ] {
        let mut input = crate::runtime::input();
        input
            .attributes
            .insert("board_pressure".into(), json!(pressure).into());
        input
            .attributes
            .insert("current_level".into(), json!(level).into());
        let decision =
            flaggo_decision::evaluate(&original, &compiled.compiled_executable, &input, None)
                .unwrap();
        assert_eq!(decision.result.as_f64(), Some(f64::from(result)));
        assert_eq!(
            serde_json::to_value(decision.evaluation).unwrap(),
            match rule {
                Some(rule) => json!({"source":"rule","rule":rule}),
                None => json!({"source":"default"}),
            }
        );
    }
}

#[test]
fn shared_canonical_corpus_passes() {
    c01_canonical();
}
