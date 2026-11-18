use std::sync::Arc;

use flaggo_decision::{
    ActiveExecutable, ActiveExecutableReader, Cancellation, ContractVersion, ContractVersionReader,
    DecisionRuntime, ReadinessProbe, StoreReadError,
};
use flaggo_expression::ExecutableCompiler;

#[derive(Clone)]
struct Source {
    contract: Arc<ContractVersion>,
    executable: Arc<ActiveExecutable>,
}

impl ContractVersionReader for Source {
    async fn get_contract(&self, name: &str, digest: &str, _: Cancellation) -> Result<Option<Arc<ContractVersion>>, StoreReadError> {
        Ok((name == self.contract.contract.name && digest == self.contract.contract_digest).then(|| self.contract.clone()))
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
    let contract = flaggo_core_conformance::contract();
    let digest = flaggo_contract::digests::contract_digest(&contract, Some(&flaggo_expression::ExpressionCompiler::default())).unwrap();
    assert_eq!(digest, "sha256:7394e6ec3d9eee8d559c4e5b44b15fb5ab7827e5307d5877f369ba0c9ded8098");
    let compiled = ExecutableCompiler::default().compile(&contract, &flaggo_contract::DecisionExecutable {
        contract_digest: digest.clone(), kind: "rules".into(), rules: vec![],
    }).unwrap();
    let source = Source {
        contract: Arc::new(ContractVersion { contract_digest: digest.clone(), contract }),
        executable: Arc::new(ActiveExecutable {
            executable_digest: compiled.executable_digest,
            executable: compiled.executable,
            checked_executable: compiled.checked_executable,
        }),
    };
    let _router = flaggo_decision_http::router(source.clone(), source.clone());
    // Compile the management composition without linking any local application or SQLite adapter.
    let _management = |authority: Arc<dyn flaggo_executable_store::ContractAuthority>| {
        flaggo_contract_application::router(flaggo_contract_application::ContractLifecycle::new(authority))
    };
    let _workspace_contract = |lease: Arc<dyn flaggo_analysis_workspace::WorkspaceLease>| {
        lease.workspace_id().to_owned()
    };
    let _evidence_contract = |store: Arc<dyn flaggo_evidence_store::EvidenceStore>| store;
    let _inbox_contract = |inbox: Arc<dyn flaggo_raw_otlp_inbox::RawOtlpInbox>| inbox;
    let cell = flaggo_evidence_store::EvidenceValue::Int64 { value: i64::MAX };
    let wire = serde_json::to_string(&cell).unwrap();
    assert_eq!(wire, r#"{"kind":"int64","value":"9223372036854775807"}"#);
    assert_eq!(serde_json::from_str::<flaggo_evidence_store::EvidenceValue>(&wire).unwrap(), cell);
    let input = flaggo_contract::strict_json::deserialize(br#"{"attributes":{
        "_random":0.5,"board_pressure":0.2,"current_level":1,
        "recent_placement_time_ms":500,"recovery_failures":0,"session_id":"consumer"
    }}"#).unwrap();
    let runtime = DecisionRuntime::new(source.clone(), source);
    let decision = runtime.decide("tetris.dropInterval", &digest, &input, None).await.unwrap();
    assert_eq!(serde_json::to_value(&decision).unwrap()["result"], 500);
    println!("Pinned core digest/evaluation and HTTP composition passed (no SQLite).");
}
