use flaggo_contract::digests::{
    ExpressionCanonicalizer, contract_digest, executable_digest, is_sha256_digest,
};
use flaggo_contract::strict_json::{self, MAXIMUM_DOCUMENT_BYTES, MAXIMUM_STRING_BYTES};
use flaggo_contract::{DecisionContract, DecisionExecutable, ValueSchema};
use serde_json::{Value, json};

fn contract() -> DecisionContract {
    strict_json::deserialize(br#"{
        "authority":{"tenant":"local","application":"demo","environment":"test"},
        "name":"demo.choice","expression_syntax":"flaggo.cel/v1",
        "attributes":[{"name":"z","schema":{"type":"integer"}},{"name":"a","schema":{"type":"string"}}],
        "result":{"schema":{"type":"object","properties":{"description":{"type":"string"}}},
                  "default":{"description":"allow","items":[1,2],"nullable":null}},
        "authoredExecutable":{"rules":[
            {"name":"first","when":{"expression":"true"},"return":{"value":{"description":"first"}}},
            {"name":"second","when":{"condition":"otherwise"},"return":{"value":null}}
        ]}
    }"#).unwrap()
}

#[test]
fn shared_valid_contract_keeps_its_published_digest() {
    #[derive(serde::Deserialize)]
    struct Fixture {
        request: FixtureRequest,
        expected: Value,
    }
    #[derive(serde::Deserialize)]
    struct FixtureRequest {
        body: Box<serde_json::value::RawValue>,
    }
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../contracts/fixtures/management/decision-contract/01-validate-valid.json"
    ))
    .unwrap();
    let model: DecisionContract =
        strict_json::deserialize(fixture.request.body.get().as_bytes()).unwrap();
    assert_eq!(
        contract_digest(&model, None).unwrap(),
        fixture.expected["body"]["contractDigest"].as_str().unwrap(),
    );
}

#[test]
fn schema_descriptions_and_named_attribute_order_are_not_semantic() {
    let first = contract();
    let mut reordered = first.clone();
    reordered.attributes.reverse();
    reordered.attributes[0].as_mut().unwrap().schema.description = Some("display only".into());
    reordered.result.schema.description = Some("display only".into());
    assert_eq!(
        contract_digest(&first, None).unwrap(),
        contract_digest(&reordered, None).unwrap()
    );
    assert!(
        first.attributes[0]
            .as_ref()
            .unwrap()
            .schema
            .description
            .is_none()
    );
}

#[test]
fn descriptions_in_values_and_rule_order_remain_semantic() {
    let first = contract();
    let digest = contract_digest(&first, None).unwrap();
    let mut changed = first.clone();
    let mut default = serde_json::to_value(&first.result.default).unwrap();
    default["description"] = json!("deny");
    changed.result.default = default.into();
    assert_ne!(digest, contract_digest(&changed, None).unwrap());
    changed = first.clone();
    changed
        .authored_executable
        .as_mut()
        .unwrap()
        .rules
        .reverse();
    assert_ne!(digest, contract_digest(&changed, None).unwrap());
    changed = first.clone();
    let mut default = serde_json::to_value(&first.result.default).unwrap();
    default["items"] = json!([2, 1]);
    changed.result.default = default.into();
    assert_ne!(digest, contract_digest(&changed, None).unwrap());
}

#[test]
fn executable_defaults_and_literal_null_are_preserved() {
    let bytes = br#"{"contractDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "rules":[{"name":"r","when":{},"return":{"value":null}}]}"#;
    let executable: DecisionExecutable = strict_json::deserialize(bytes).unwrap();
    assert_eq!(executable.kind, "rules");
    assert_eq!(
        serde_json::to_value(&executable).unwrap()["rules"][0]["when"],
        json!({})
    );
    assert_eq!(
        serde_json::to_value(&executable).unwrap()["rules"][0]["return"],
        json!({"value":null})
    );
    assert!(is_sha256_digest(
        &executable_digest(&executable, None).unwrap()
    ));
    assert!(!is_sha256_digest(&format!("sha256:{}", "A".repeat(64))));
}

#[test]
fn executable_digest_preserves_null_rules_without_validating_them() {
    let executable: DecisionExecutable = strict_json::deserialize(
        br#"{"contractDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rules":[null]}"#
    ).unwrap();
    assert_eq!(
        executable_digest(&executable, None).unwrap(),
        "sha256:1021e0d8e9273bdd02e9e158e0d4dd4b1cdde5bf3628ff5b6cc0cdeafa52c588"
    );
    assert_eq!(
        serde_json::to_value(&executable).unwrap()["rules"],
        json!([null])
    );
}

#[test]
fn executable_digest_canonicalizes_non_null_rules_without_dropping_null_positions() {
    struct Trim;
    impl ExpressionCanonicalizer for Trim {
        fn canonicalize(&self, expression: &str) -> Result<String, String> {
            Ok(expression.trim().to_owned())
        }
    }
    let executable: DecisionExecutable = strict_json::deserialize(
        br#"{"contractDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rules":[null,{"name":"r","when":{"expression":" true "},"return":{"expression":" 1 "}},null]}"#
    ).unwrap();
    let expected: DecisionExecutable = strict_json::deserialize(
        br#"{"contractDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rules":[null,{"name":"r","when":{"expression":"true"},"return":{"expression":"1"}},null]}"#
    ).unwrap();
    assert_eq!(
        executable_digest(&executable, Some(&Trim)).unwrap(),
        executable_digest(&expected, None).unwrap(),
    );
    assert_eq!(executable.rules.len(), 3);
    assert!(executable.rules[0].is_none() && executable.rules[2].is_none());
    assert_eq!(
        executable.rules[1]
            .as_ref()
            .unwrap()
            .when
            .expression
            .as_deref(),
        Some(" true ")
    );
}

#[test]
fn literal_json_deserialization_still_supports_materialized_serde_values() {
    for value in [
        json!(null),
        json!(true),
        json!(1),
        json!(-0.0),
        json!("text"),
        json!({"nested":[1, true, null, "text"]}),
    ] {
        let literal: flaggo_contract::JsonValue = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(literal).unwrap(), value);
    }
}

#[test]
fn raw_literal_decoding_does_not_bypass_serdes_recursion_limit() {
    for depth in [126, 127, 128, 129] {
        let text = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(
            serde_json::from_str::<flaggo_contract::JsonValue>(&text).is_ok(),
            serde_json::from_str::<Value>(&text).is_ok(),
            "depth {depth}",
        );
    }
}

#[test]
fn strict_json_rejects_duplicates_unknown_fields_and_case_mismatches() {
    for bytes in [
        br#"{"type":"string","type":"number"}"#.as_slice(),
        br#"{"type":"string","pattern":"x"}"#,
        br#"{"Type":"string"}"#,
        br#"{"type":"string",}"#,
        br#"{"type":"string"/*comment*/}"#,
    ] {
        assert!(strict_json::deserialize::<ValueSchema>(bytes).is_err());
    }
    assert!(strict_json::validate(br#"{"a":1,"\u0061":2}"#).is_err());
    assert!(strict_json::validate(br#"{"outer":[{"a":1,"a":2}]}"#).is_err());
}

#[test]
fn strict_json_limits_use_decoded_utf8_bytes_and_container_depth() {
    for (characters, accepted) in [
        (MAXIMUM_STRING_BYTES / 2, true),
        (MAXIMUM_STRING_BYTES / 2 + 1, false),
    ] {
        let document = serde_json::to_vec(&"\u{e9}".repeat(characters)).unwrap();
        assert_eq!(strict_json::validate(&document).is_ok(), accepted);
    }
    for (depth, accepted) in [(16, true), (17, false)] {
        let document = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(strict_json::validate(document.as_bytes()).is_ok(), accepted);
    }
    let mut document = vec![b' '; MAXIMUM_DOCUMENT_BYTES];
    document[0] = b'0';
    assert!(strict_json::validate(&document).is_ok());
    document.push(b' ');
    assert!(strict_json::validate(&document).is_err());
}

#[test]
fn token_validation_does_not_silently_coerce_large_json_numbers() {
    assert!(strict_json::validate(br#"{"value":1e400}"#).is_ok());
    assert!(strict_json::validate(br#"{"value":NaN}"#).is_err());
    assert!(strict_json::validate(br#""\ud800""#).is_err());
}

#[test]
fn invalid_unicode_keeps_its_distinct_legacy_fault_category() {
    for document in [
        br#""\ud800""#.as_slice(),
        br#""\udc00""#,
        br#"{"\ud800":1}"#,
        br#"{"items":["\ud800"]}"#,
    ] {
        assert!(matches!(
            strict_json::validate(document),
            Err(flaggo_contract::ContractError::InvalidUnicode(_))
        ));
    }
    for document in [br#""\uZZZZ""#.as_slice(), br#"{"\uZZZZ":1}"#, br#""\x20""#] {
        assert!(matches!(
            strict_json::validate(document),
            Err(flaggo_contract::ContractError::Json(_))
        ));
    }
    assert!(strict_json::validate(br#""\ud83d\ude00""#).is_ok());
}

#[test]
fn evidence_discriminator_order_is_preserved_without_reordering_input() {
    use flaggo_contract::EvidenceSource;
    let valid = br#"{"kind":"log","scope":"s","name":"n","correlation":{}}"#;
    assert!(strict_json::deserialize::<EvidenceSource>(valid).is_ok());
    for invalid in [
        br#"{"scope":"s","name":"n","correlation":{},"kind":"log"}"#.as_slice(),
        br#"{"scope":"s","name":"n","correlation":{}}"#,
        br#"{"kind":"unknown","scope":"s","name":"n","correlation":{}}"#,
        br#"{"kind":"log","scope":"s","name":"n","correlation":{},"extra":1}"#,
    ] {
        assert!(strict_json::deserialize::<EvidenceSource>(invalid).is_err());
    }
}
