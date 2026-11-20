use std::sync::Arc;

use flaggo_decision::{
    ActiveExecutable, ActiveExecutableReader, Cancellation, ContractVersion, ContractVersionReader,
    DecisionRuntime, ReadinessProbe, StoreReadError,
};
use flaggo_expression::ExecutableCompiler;
use flaggo_contract::ContractResourceId;

#[derive(Clone)]
struct Source {
    contract: Arc<ContractVersion>,
    executable: Arc<ActiveExecutable>,
}

impl ContractVersionReader for Source {
    async fn get_contract(&self, resource: &ContractResourceId, digest: &str, _: Cancellation) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
        Ok((*resource == ContractResourceId::try_from(&self.contract.contract).unwrap() && digest == self.contract.contract_digest).then(|| self.contract.clone()))
    }
}

impl ActiveExecutableReader for Source {
    async fn get_active(&self, digest: &str, _: Cancellation) -> Result<Option<Arc<ActiveExecutable>>, StoreReadError> {
        Ok((digest == self.contract.contract_digest).then(|| self.executable.clone()))
    }
}

impl ReadinessProbe for Source {
    async fn is_available(&self, _: Cancellation) -> Result<bool, StoreReadError> { Ok(true) }
}

#[tokio::main]
async fn main() {
    flaggo_core_conformance::canonical::c01_canonical();
    flaggo_core_conformance::policy::c03_c05_policy();
    flaggo_core_conformance::policy::persisted_artifact_integrity();
    flaggo_core_conformance::policy::persisted_contract_integrity();
    flaggo_core_conformance::evidence_policy::c09_projection_policy();
    let contract = flaggo_core_conformance::contract();
    let resource = ContractResourceId::try_from(&contract).unwrap();
    let digest = flaggo_contract::digests::contract_digest(&contract, Some(&flaggo_expression::ExpressionCompiler::default())).unwrap();
    assert_eq!(digest, "sha256:7394e6ec3d9eee8d559c4e5b44b15fb5ab7827e5307d5877f369ba0c9ded8098");
    let compiled = ExecutableCompiler::default().compile(&contract, &flaggo_contract::DecisionExecutable {
        contract_digest: digest.clone(), kind: "rules".into(), rules: vec![],
    }).unwrap();
    let expected = flaggo_decision::evaluate(&contract, &compiled.compiled_executable, &flaggo_core_conformance::runtime::input(), None).unwrap();
    let source = Source {
        contract: Arc::new(ContractVersion { contract_digest: digest.clone(), contract }),
        executable: Arc::new(ActiveExecutable {
            executable_digest: compiled.executable_digest,
            executable: compiled.executable,
            checked_executable: compiled.checked_executable,
        }),
    };
    let security: Arc<dyn flaggo_service_hosting::RequestSecurity> = Arc::new(flaggo_service_hosting::TrustedLocal);
    let principal = flaggo_service_hosting::Principal { subject: "consumer".into() };
    let admission: Result<flaggo_service_hosting::RequestAdmission, flaggo_service_hosting::RequestAdmissionError> =
        security.admit_authorized(&principal, &resource.scope, Some(&resource.name), flaggo_service_hosting::Operation::Evaluate).await;
    assert_eq!(admission, Ok(flaggo_service_hosting::RequestAdmission::Accepted));
    assert_eq!(security.admit_authorized_otlp(&principal, &[]).await, Ok(flaggo_service_hosting::RequestAdmission::Accepted));
    assert_eq!(security.admit_authorized_otlp(&principal, &[resource.scope.clone()]).await, Ok(flaggo_service_hosting::RequestAdmission::Accepted));
    let _router = flaggo_decision_http::router(source.clone(), source.clone(), security);
    // Compile the management composition without linking any local application or SQLite adapter.
    let _management = |authority: Arc<dyn flaggo_executable_store::ContractAuthority>| {
        flaggo_contract_application::router(flaggo_contract_application::ContractLifecycle::new(authority), Arc::new(flaggo_service_hosting::TrustedLocal))
    };
    let _workspace_contract = |lease: Arc<dyn flaggo_analysis_workspace::WorkspaceLease>| {
        lease.workspace_id().to_owned()
    };
    let _analysis_composition = |
        contracts: Arc<dyn flaggo_analysis_application::ContractService>,
        evidence: Arc<dyn flaggo_evidence_store::EvidenceAnalysisStore>,
        execution: Arc<dyn flaggo_analysis_execution::ExecutionProvider>,
        limits: flaggo_evidence_store::EvidenceQueryLimits,
    | {
        flaggo_analysis_application::CapabilityFactory::new(
            contracts, evidence, limits,
            Arc::new(flaggo_analysis_application::SystemClock), execution,
        )
    };
    let _evidence_contract = |store: Arc<dyn flaggo_evidence_store::EvidenceStore>| store;
    let _projection_catalog = flaggo_evidence_materialization::CompiledContractCatalog::empty();
    let _inbox_contract = |inbox: Arc<dyn flaggo_raw_otlp_inbox::RawOtlpInbox>| inbox;
    let _ingestion = |inbox: Arc<dyn flaggo_raw_otlp_inbox::RawOtlpInbox>| {
        flaggo_otlp_http::router_with_private_health(inbox, Default::default(), Arc::new(flaggo_service_hosting::TrustedLocal))
    };
    let cell = flaggo_evidence_store::EvidenceValue::Int64 { value: i64::MAX };
    let wire = serde_json::to_string(&cell).unwrap();
    assert_eq!(wire, r#"{"kind":"int64","value":"9223372036854775807"}"#);
    assert_eq!(serde_json::from_str::<flaggo_evidence_store::EvidenceValue>(&wire).unwrap(), cell);
    let input = flaggo_contract::strict_json::deserialize(br#"{"attributes":{
        "_random":0.5,"board_pressure":0.2,"current_level":1,
        "recent_placement_time_ms":500,"recovery_failures":0,"session_id":"consumer"
    }}"#).unwrap();
    let runtime = DecisionRuntime::new(source.clone(), source);
    flaggo_core_conformance::runtime::assert_exact_decision(&runtime, &resource, &digest, &expected).await;
    let decision = runtime.decide(&resource, &digest, &input, None).await.unwrap();
    assert_eq!(serde_json::to_value(&decision).unwrap()["result"], 500);
    println!("Pinned core digest/evaluation, projection policy, default single/batch admission and retention-independent HTTP composition passed (no SQLite).");
}
