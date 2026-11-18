//! Exact-version behavior and integrity outcomes for independently supplied readers.
use std::sync::Arc;

use async_trait::async_trait;
use flaggo_contract::{
    ContractResourceId, DecisionExecutable, RuntimeDecision, RuntimeInput, strict_json,
};
use flaggo_contract_application::ContractLifecycle;
use flaggo_decision::{ActiveExecutableReader, ContractVersionReader, DecisionRuntime};
use flaggo_executable_store::ContractAuthority;
use flaggo_expression::ExecutableCompiler;
use serde_json::json;

pub const FIXTURE_REVISION: &str = "flaggo.runtime-conformance/v1";

#[async_trait]
pub trait RuntimeFixture {
    type Contracts: ContractVersionReader;
    type Executables: ActiveExecutableReader;
    fn authority(&self) -> Arc<dyn ContractAuthority>;
    fn readers(&self) -> (Self::Contracts, Self::Executables);
    fn unavailable(&self, unavailable: bool);
    /// Corrupt the persisted checked artifact, not only a transient result.
    async fn corrupt_active(&self, contract_digest: &str);
}

pub fn input() -> RuntimeInput {
    strict_json::deserialize(
        br#"{"attributes":{
        "_random":0.5,"board_pressure":0.2,"current_level":1,
        "recent_placement_time_ms":500,"recovery_failures":0,"session_id":"conformance"
    }}"#,
    )
    .unwrap()
}

pub async fn assert_exact_decision<C: ContractVersionReader, E: ActiveExecutableReader>(
    runtime: &DecisionRuntime<C, E>,
    resource: &ContractResourceId,
    digest: &str,
    expected: &RuntimeDecision,
) {
    let actual = runtime
        .decide(resource, digest, &input(), None)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

pub async fn c02_exact_runtime(fixture: &impl RuntimeFixture) {
    let service = ContractLifecycle::new(fixture.authority());
    let mut contract = crate::contract();
    contract.authored_executable = None;
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let first = service.deploy(&resource, &contract).await.unwrap().0;
    let compiled = ExecutableCompiler::default()
        .compile(
            &contract,
            &DecisionExecutable {
                contract_digest: first.contract_digest.clone(),
                kind: "rules".into(),
                rules: vec![],
            },
        )
        .unwrap();
    let expected =
        flaggo_decision::evaluate(&contract, &compiled.compiled_executable, &input(), None)
            .unwrap();
    let (contracts, executables) = fixture.readers();
    let runtime = DecisionRuntime::new(contracts, executables);
    assert_exact_decision(&runtime, &resource, &first.contract_digest, &expected).await;
    let mut newer = contract.clone();
    newer.result.default = json!(600).into();
    let second = service.deploy(&resource, &newer).await.unwrap().0;
    assert_ne!(first.contract_digest, second.contract_digest);
    // Management current is not a runtime selector.
    assert_exact_decision(&runtime, &resource, &first.contract_digest, &expected).await;
    let mut another_scope = resource.clone();
    another_scope.scope.tenant = "different-tenant".into();
    assert_eq!(
        runtime
            .decide(&another_scope, &first.contract_digest, &input(), None)
            .await
            .unwrap_err()
            .telemetry_classification(),
        ("not_found", "not_found")
    );
    fixture.unavailable(true);
    assert_eq!(
        runtime
            .decide(&resource, &first.contract_digest, &input(), None)
            .await
            .unwrap_err()
            .telemetry_classification(),
        ("unavailable", "dependency")
    );
    fixture.unavailable(false);
    assert_exact_decision(&runtime, &resource, &first.contract_digest, &expected).await;
    fixture.corrupt_active(&first.contract_digest).await;
    assert_eq!(
        runtime
            .decide(&resource, &first.contract_digest, &input(), None)
            .await
            .unwrap_err()
            .telemetry_classification(),
        ("failure", "integrity")
    );
    let (contracts, executables) = fixture.readers();
    let fresh = DecisionRuntime::new(contracts, executables);
    assert_eq!(
        fresh
            .decide(&resource, &first.contract_digest, &input(), None)
            .await
            .unwrap_err()
            .telemetry_classification(),
        ("failure", "integrity")
    );
}
