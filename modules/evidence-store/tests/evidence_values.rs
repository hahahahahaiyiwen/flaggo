use flaggo_evidence_store::EvidenceValue;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    valid: Vec<Value>,
    invalid: Vec<Value>,
    invalid_json_text: Vec<Value>,
}

#[test]
fn canonical_cells_preserve_the_shared_wire_corpus_and_reject_invalid_values() {
    let corpus: Corpus = serde_json::from_slice(include_bytes!(
        "../../../contracts/fixtures/evidence-value-v1.json"
    ))
    .unwrap();
    for wire in corpus.valid {
        let decoded: EvidenceValue = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
    for wire in corpus.invalid.into_iter().chain(corpus.invalid_json_text) {
        assert!(
            serde_json::from_value::<EvidenceValue>(wire.clone()).is_err(),
            "accepted invalid cell: {wire}"
        );
    }
}

#[test]
fn encoding_preserves_integer_precision_and_float_payload_bits() {
    assert_eq!(
        serde_json::to_string(&EvidenceValue::Int64 { value: i64::MAX }).unwrap(),
        r#"{"kind":"int64","value":"9223372036854775807"}"#
    );
    for bits in [
        (-0.0_f64).to_bits(),
        f64::INFINITY.to_bits(),
        0x7ff8_0000_0000_0042,
        0xfff8_0000_0000_0042,
    ] {
        let value = EvidenceValue::Float64 { bits };
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            serde_json::from_slice::<EvidenceValue>(&bytes).unwrap(),
            value
        );
    }
    assert!(
        serde_json::to_vec(&EvidenceValue::Json {
            value: "invalid json".to_owned()
        })
        .is_err()
    );
}
