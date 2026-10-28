use super::*;
use crate::{DecisionError, evaluate};
use flaggo_contract::{EvaluationProvenance, RuleReturn, strict_json};
use flaggo_expression::ExpressionCompiler;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Clone)]
struct Reader {
    version: Arc<ContractVersion>,
    active: Arc<Mutex<Option<Arc<ActiveExecutable>>>>,
    reads: Arc<AtomicUsize>,
}

impl ContractVersionReader for Reader {
    async fn get_contract(
        &self,
        name: &str,
        digest: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
        Ok(
            (name == self.version.contract.name && digest == self.version.contract_digest)
                .then(|| Arc::clone(&self.version)),
        )
    }
}

impl ActiveExecutableReader for Reader {
    async fn get_active(
        &self,
        _: &str,
        _: Cancellation,
    ) -> Result<Option<Arc<ActiveExecutable>>, StoreReadError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(self.active.lock().unwrap().clone())
    }
}

fn contract() -> DecisionContract {
    strict_json::deserialize(
        br#"{
        "authority":{"tenant":"local","application":"runtime","environment":"test"},
        "name":"runtime.test","expression_syntax":"flaggo.cel/v1",
        "attributes":[
            {"name":"x","schema":{"type":"integer"}},
            {"name":"y","schema":{"type":"integer"}}
        ],
        "result":{"schema":{"type":"integer","minimum":0},"default":99}
    }"#,
    )
    .unwrap()
}

fn reader() -> Reader {
    let contract = contract();
    let digest =
        flaggo_contract::digests::contract_digest(&contract, Some(&ExpressionCompiler::default()))
            .unwrap();
    Reader {
        version: Arc::new(ContractVersion {
            contract_digest: digest,
            contract,
        }),
        active: Arc::new(Mutex::new(None)),
        reads: Arc::new(AtomicUsize::new(0)),
    }
}

fn active(reader: &Reader, rules: serde_json::Value) -> Arc<ActiveExecutable> {
    let executable = strict_json::deserialize(
        &serde_json::to_vec(&json!({
            "contractDigest":reader.version.contract_digest, "rules":rules,
        }))
        .unwrap(),
    )
    .unwrap();
    let compilation = ExecutableCompiler::default()
        .compile(&reader.version.contract, &executable)
        .unwrap();
    Arc::new(ActiveExecutable {
        executable_digest: compilation.executable_digest,
        executable: compilation.executable,
        checked_executable: compilation.checked_executable,
    })
}

fn literal(name: &str, result: i64) -> serde_json::Value {
    json!({"name":name,"when":{"expression":" true "},"return":{"value":result}})
}

fn input(attributes: serde_json::Value) -> RuntimeInput {
    strict_json::deserialize(&serde_json::to_vec(&json!({"attributes":attributes})).unwrap())
        .unwrap()
}

#[tokio::test]
async fn runtime_observes_each_activation_and_keeps_cached_executables_immutable() {
    let reader = reader();
    let first = active(&reader, json!([literal("first", 1)]));
    let second = active(&reader, json!([literal("second", 2)]));
    let runtime = DecisionRuntime::new(reader.clone(), reader.clone());
    *reader.active.lock().unwrap() = Some(Arc::clone(&first));
    let request = input(json!({"_random":0.5}));
    let one = runtime
        .decide(
            "runtime.test",
            &reader.version.contract_digest,
            &request,
            None,
        )
        .await
        .unwrap();
    *reader.active.lock().unwrap() = Some(Arc::clone(&second));
    let two = runtime
        .decide(
            "runtime.test",
            &reader.version.contract_digest,
            &request,
            None,
        )
        .await
        .unwrap();
    assert_eq!(one.executable_digest, first.executable_digest);
    assert_eq!(one.result, json!(1).into());
    assert_eq!(two.executable_digest, second.executable_digest);
    assert_eq!(two.result, json!(2).into());
    assert_eq!(reader.reads.load(Ordering::Relaxed), 2);
    *reader.active.lock().unwrap() = None;
    assert!(matches!(
        runtime
            .decide(
                "runtime.test",
                &reader.version.contract_digest,
                &request,
                None
            )
            .await,
        Err(DecisionError::ActiveExecutableNotFound(_))
    ));
}

#[tokio::test]
async fn validates_identity_then_contract_then_complete_input_before_active_read() {
    let reader = reader();
    let runtime = DecisionRuntime::new(reader.clone(), reader.clone());
    let invalid =
        strict_json::deserialize(br#"{"attributes":{"x":"wrong","_unknown":1,"undeclared":2}}"#)
            .unwrap();
    assert!(matches!(
        runtime.decide("bad name", "bad", &invalid, None).await,
        Err(DecisionError::InvalidIdentity(_))
    ));
    assert!(matches!(
        runtime
            .decide(
                "other.name",
                &reader.version.contract_digest,
                &invalid,
                None
            )
            .await,
        Err(DecisionError::ContractNotFound { .. })
    ));
    let error = runtime
        .decide(
            "runtime.test",
            &reader.version.contract_digest,
            &invalid,
            None,
        )
        .await
        .unwrap_err();
    let DecisionError::InputValidation(issues) = error else {
        panic!("wrong error")
    };
    assert_eq!(
        issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<Vec<_>>(),
        [
            "invalid-internal-attribute",
            "invalid-value-type",
            "unknown-internal-attribute",
            "undeclared-attribute"
        ]
    );
    assert_eq!(reader.reads.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn rejects_cross_contract_active_binding_before_materialization() {
    let reader = reader();
    let mut other = Arc::try_unwrap(active(&reader, json!([literal("r", 1)])))
        .ok()
        .unwrap();
    other.executable.contract_digest = format!("sha256:{}", "0".repeat(64));
    *reader.active.lock().unwrap() = Some(Arc::new(other));
    let runtime = DecisionRuntime::new(reader.clone(), reader.clone());
    let error = runtime
        .decide(
            "runtime.test",
            &reader.version.contract_digest,
            &input(json!({"_random":0.5})),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "The active executable is bound to a different contract digest."
    );
    assert_eq!(error.telemetry_classification(), ("failure", "integrity"));
}

#[test]
fn materialization_failure_is_evicted_and_same_identity_can_retry() {
    let reader = reader();
    let good = active(&reader, json!([literal("r", 1)]));
    let mut bad = ActiveExecutable {
        executable: good.executable.clone(),
        executable_digest: good.executable_digest.clone(),
        checked_executable: good.checked_executable.clone(),
    };
    bad.checked_executable.rules[0]
        .as_mut()
        .unwrap()
        .predicate
        .checked_ast = "AA==".into();
    let runtime = DecisionRuntime::new(reader.clone(), reader.clone());
    assert!(matches!(
        runtime.materialize(&reader.version, &bad),
        Err(DecisionError::Integrity { .. })
    ));
    assert!(runtime.cache.lock().unwrap().entries.is_empty());
    let compiled = runtime.materialize(&reader.version, &good).unwrap();
    assert_eq!(compiled.executable_digest(), good.executable_digest);
    let cached = runtime.materialize(&reader.version, &good).unwrap();
    assert!(Arc::ptr_eq(&compiled, &cached));
}

#[test]
fn changed_checked_digest_does_not_reuse_a_cached_executable() {
    let reader = reader();
    let good = active(&reader, json!([literal("r", 1)]));
    let runtime = DecisionRuntime::new(reader.clone(), reader.clone());
    runtime.materialize(&reader.version, &good).unwrap();
    let mut changed = ActiveExecutable {
        executable: good.executable.clone(),
        executable_digest: good.executable_digest.clone(),
        checked_executable: good.checked_executable.clone(),
    };
    changed.checked_executable.rules[0]
        .as_mut()
        .unwrap()
        .predicate
        .checked_ast_digest = format!("sha256:{}", "0".repeat(64));
    assert!(matches!(
        runtime.materialize(&reader.version, &changed),
        Err(DecisionError::Integrity { .. })
    ));
    assert_eq!(runtime.cache.lock().unwrap().entries.len(), 1);
}

#[test]
fn cache_has_exact_1024_entry_fifo_bound_and_hits_do_not_grow_the_queue() {
    let reader = reader();
    let runtime = DecisionRuntime::new(reader.clone(), reader.clone());
    let first = active(&reader, json!([literal("r0", 0)]));
    let first_key = {
        runtime.materialize(&reader.version, &first).unwrap();
        runtime
            .cache
            .lock()
            .unwrap()
            .order
            .front()
            .unwrap()
            .0
            .clone()
    };
    for index in 1..MAXIMUM_COMPILED_EXECUTABLES {
        let candidate = active(
            &reader,
            json!([literal(&format!("r{index}"), index as i64)]),
        );
        runtime.materialize(&reader.version, &candidate).unwrap();
    }
    assert_eq!(runtime.cache.lock().unwrap().entries.len(), 1024);
    assert!(
        runtime
            .cache
            .lock()
            .unwrap()
            .entries
            .contains_key(&first_key)
    );
    for _ in 0..16 {
        runtime.materialize(&reader.version, &first).unwrap();
    }
    assert_eq!(runtime.cache.lock().unwrap().order.len(), 1024);
    let last = active(&reader, json!([literal("last", 0)]));
    runtime.materialize(&reader.version, &last).unwrap();
    let cache = runtime.cache.lock().unwrap();
    assert_eq!(cache.entries.len(), 1024);
    assert_eq!(cache.order.len(), 1024);
    assert!(!cache.entries.contains_key(&first_key));
}

#[test]
fn concurrent_requests_share_one_immutable_materialized_program() {
    let reader = reader();
    let active = active(&reader, json!([literal("r", 7)]));
    let runtime = DecisionRuntime::new(reader.clone(), reader.clone());
    let barrier = std::sync::Barrier::new(16);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..16)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    runtime.materialize(&reader.version, &active).unwrap()
                })
            })
            .collect();
        let values: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert!(values.iter().all(|value| Arc::ptr_eq(value, &values[0])));
    });
    assert_eq!(runtime.cache.lock().unwrap().entries.len(), 1);
    assert_eq!(runtime.cache.lock().unwrap().order.len(), 1);
}

#[test]
fn missing_return_reference_skips_the_predicate_even_when_it_would_fail() {
    let reader = reader();
    let active = active(
        &reader,
        json!([
            {"name":"skip","when":{"expression":"1 / attributes.x > 0"},
             "return":{"expression":"attributes.y"}},
            literal("next", 7),
        ]),
    );
    let compiled = DecisionRuntime::new(reader.clone(), reader.clone())
        .materialize(&reader.version, &active)
        .unwrap();
    let result = evaluate(
        &reader.version.contract,
        &compiled,
        &input(json!({"_random":0.5,"x":0})),
        None,
    )
    .unwrap();
    assert_eq!(result.result, json!(7).into());
    assert_eq!(
        result.evaluation,
        EvaluationProvenance::Rule {
            rule: "next".into()
        }
    );
}

#[test]
fn compiler_detaches_mutable_documents_and_rechecks_dynamic_results() {
    let contract = contract();
    let digest =
        flaggo_contract::digests::contract_digest(&contract, Some(&ExpressionCompiler::default()))
            .unwrap();
    let executable = strict_json::deserialize(
        &serde_json::to_vec(&json!({
            "contractDigest":digest, "rules":[literal("r", 7)],
        }))
        .unwrap(),
    )
    .unwrap();
    let mut compilation = ExecutableCompiler::default()
        .compile(&contract, &executable)
        .unwrap();
    compilation.executable.rules[0].as_mut().unwrap().result = RuleReturn::Value(json!(8).into());
    let result = evaluate(
        &contract,
        &compilation.compiled_executable,
        &input(json!({"_random":0.5})),
        None,
    )
    .unwrap();
    assert_eq!(result.result, json!(7).into());
    let reader = reader();
    let active = active(
        &reader,
        json!([
            {"name":"r","when":{"expression":"true"},"return":{"expression":"attributes.x"}}
        ]),
    );
    let compiled = ExecutableCompiler::default()
        .materialize(
            &contract,
            &reader.version.contract_digest,
            &active.executable,
            &active.executable_digest,
            &active.checked_executable,
        )
        .unwrap();
    let error = evaluate(
        &contract,
        &compiled,
        &input(json!({"_random":0.5,"x":-1})),
        None,
    )
    .unwrap_err();
    assert!(matches!(error, DecisionError::ResultValidation { .. }));
    assert_eq!(error.telemetry_classification(), ("failure", "integrity"));
}

#[test]
fn cancellation_is_checked_per_rule_but_not_for_an_empty_default() {
    let reader = reader();
    for (rules, cancelled) in [(json!([literal("r", 7)]), true), (json!([]), false)] {
        let active = active(&reader, rules);
        let compiled = ExecutableCompiler::default()
            .materialize(
                &reader.version.contract,
                &reader.version.contract_digest,
                &active.executable,
                &active.executable_digest,
                &active.checked_executable,
            )
            .unwrap();
        let result = evaluate(
            &reader.version.contract,
            &compiled,
            &input(json!({"_random":0.5})),
            Some(Arc::new(AtomicBool::new(true))),
        );
        if cancelled {
            assert!(matches!(result, Err(DecisionError::Cancelled)));
        } else {
            assert_eq!(result.unwrap().evaluation.source(), "default");
        }
    }
}

#[test]
fn default_result_is_validated_even_without_any_rules() {
    let reader = reader();
    let active = active(&reader, json!([]));
    let compiled = ExecutableCompiler::default()
        .materialize(
            &reader.version.contract,
            &reader.version.contract_digest,
            &active.executable,
            &active.executable_digest,
            &active.checked_executable,
        )
        .unwrap();
    let mut changed = reader.version.contract.clone();
    changed.result.default = json!(-1).into();
    let error = evaluate(&changed, &compiled, &input(json!({"_random":0.5})), None).unwrap_err();
    let DecisionError::ResultValidation { rule, issues } = error else {
        panic!("wrong error")
    };
    assert!(rule.is_none());
    assert_eq!(issues[0].path, "/result");
}
