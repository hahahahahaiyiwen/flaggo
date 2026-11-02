use flaggo_contract::{
    ContractError, EvidenceSource, JsonNumber, JsonValue, ValueSchema, strict_json, value_schema,
};
use serde::Deserialize;
use serde_json::json;

fn schema(value: serde_json::Value) -> ValueSchema {
    serde_json::from_value(value).unwrap()
}

#[test]
fn raw_numbers_round_trip_without_becoming_global_serde_features() {
    for token in [
        "-0",
        "-0.0",
        "-0e0",
        "1.00",
        "1e400",
        "-1e400",
        "1e-400",
        "9007199254740993",
        "1.7976931348623159e308",
    ] {
        let value: JsonValue = strict_json::deserialize(token.as_bytes()).unwrap();
        assert_eq!(
            strict_json::serialize(&value).unwrap(),
            token.as_bytes(),
            "{token}"
        );
        let nested = format!("{{\"z\":[{token}],\"a\":true}}");
        let value: JsonValue = strict_json::deserialize(nested.as_bytes()).unwrap();
        assert_eq!(
            strict_json::serialize(&value).unwrap(),
            nested.as_bytes(),
            "{token}"
        );
    }
    assert!(serde_json::from_str::<serde_json::Value>("1e400").is_err());
    assert!(serde_json::from_str::<JsonNumber>("true").is_err());
    assert!(serde_json::from_str::<JsonNumber>("\"1\"").is_err());
}

#[test]
fn raw_number_validation_is_not_a_deserialization_fault() {
    for token in ["1e400", "-1e400", "1.7976931348623159e308"] {
        let value: JsonValue = strict_json::deserialize(token.as_bytes()).unwrap();
        assert!(!value.as_f64().unwrap().is_finite());
        for kind in ["number", "integer", "string"] {
            let mut issues = Vec::new();
            value_schema::validate_value(
                &schema(json!({"type": kind})),
                &value,
                "/value",
                &mut issues,
            )
            .unwrap();
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0].code, "invalid-value-type");
        }
        assert!(matches!(
            strict_json::to_value(&value),
            Err(ContractError::Json(_))
        ));
    }
    assert_eq!(
        strict_json::deserialize::<JsonValue>(b"-0")
            .unwrap()
            .as_i64(),
        Some(0)
    );
    for token in ["-0.0", "-0e0"] {
        assert_eq!(
            strict_json::deserialize::<JsonValue>(token.as_bytes())
                .unwrap()
                .as_i64(),
            None
        );
    }
}

#[test]
fn typed_double_overflow_is_allowed_until_serialization() {
    for keyword in [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
    ] {
        let text = format!("{{\"type\":\"number\",\"{keyword}\":1e400}}");
        let value: ValueSchema = strict_json::deserialize(text.as_bytes()).unwrap();
        let mut issues = Vec::new();
        value_schema::validate_schema(&value, "/schema", &mut issues);
        assert!(issues.is_empty(), "{keyword}");
        assert!(matches!(
            strict_json::serialize(&value),
            Err(ContractError::Serialization(_))
        ));
        assert!(matches!(
            strict_json::to_value(&value),
            Err(ContractError::Serialization(_))
        ));
    }
    for value in [json!("1"), json!(true), json!({})] {
        assert!(
            strict_json::deserialize::<ValueSchema>(
                format!("{{\"type\":\"number\",\"minimum\":{value}}}").as_bytes()
            )
            .is_err()
        );
    }
}

#[test]
fn unique_item_canonicalization_cannot_bypass_number_conversion() {
    let schema = schema(json!({"type":"array", "uniqueItems":true}));
    for raw in ["[1e400]", "[{\"n\":1e400}]"] {
        let value: JsonValue = strict_json::deserialize(raw.as_bytes()).unwrap();
        assert!(matches!(
            value_schema::validate_value(&schema, &value, "/value", &mut Vec::new()),
            Err(ContractError::Json(_))
        ));
    }
    for raw in [
        "[1,1.0]",
        "[-0,0e0]",
        "[{\"a\":1,\"b\":2},{\"b\":2.0,\"a\":1.0}]",
    ] {
        let value: JsonValue = strict_json::deserialize(raw.as_bytes()).unwrap();
        let mut issues = Vec::new();
        value_schema::validate_value(&schema, &value, "/value", &mut issues).unwrap();
        assert_eq!(issues[0].code, "array-items-not-unique");
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    #[allow(dead_code)]
    sources: Vec<EvidenceSource>,
    #[allow(dead_code)]
    tail: bool,
}

#[test]
fn missing_or_late_discriminators_have_a_distinct_typed_fault() {
    for source in [
        "{}",
        r#"{"scope":"s","kind":"log"}"#,
        r#"{"extra":true,"kind":"log"}"#,
        r#"{"$type":"log","kind":"log"}"#,
    ] {
        assert!(matches!(
            strict_json::deserialize::<EvidenceSource>(source.as_bytes()),
            Err(ContractError::MissingTypeDiscriminator)
        ));
        let nested = format!("{{\"sources\":[{source}],\"tail\":true}}");
        assert!(matches!(
            strict_json::deserialize::<Envelope>(nested.as_bytes()),
            Err(ContractError::MissingTypeDiscriminator)
        ));
    }
    for source in [
        r#"{"kind":"other"}"#,
        r#"{"kind":null}"#,
        r#"{"kind":1}"#,
        r#"{"$id":"a","kind":"log"}"#,
        r#"{"$ref":"a","kind":"log"}"#,
        r#"{"$values":[],"kind":"log"}"#,
        "[]",
        "null",
    ] {
        assert!(
            matches!(
                strict_json::deserialize::<EvidenceSource>(source.as_bytes()),
                Err(ContractError::Json(_))
            ),
            "{source}"
        );
    }
    assert!(strict_json::deserialize::<JsonValue>(br#"{"source":{}}"#).is_ok());
}

#[test]
fn native_fault_tracking_preserves_failure_order_and_has_no_cross_request_state() {
    let before = br#"{"tail":1,"sources":[{}]}"#;
    let after = br#"{"sources":[{}],"tail":1}"#;
    for _ in 0..8 {
        assert!(matches!(
            strict_json::deserialize::<Envelope>(before),
            Err(ContractError::Json(_))
        ));
        assert!(matches!(
            strict_json::deserialize::<Envelope>(after),
            Err(ContractError::MissingTypeDiscriminator)
        ));
        assert!(matches!(
            strict_json::deserialize::<Envelope>(
                br#"{"sources":[{"kind":"log","kind":"log"}],"tail":true}"#
            ),
            Err(ContractError::Json(_))
        ));
        assert!(strict_json::deserialize::<Envelope>(br#"{"sources":[{"kind":"log","scope":"s","name":"n","correlation":{}}],"tail":true}"#).is_ok());
    }
}
