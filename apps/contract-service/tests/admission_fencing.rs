use std::sync::Arc;
use std::time::Duration;

use flaggo_analysis_workspace::LocalWorkspaceProvider;
use flaggo_contract::{CandidateOwnership, ContractResourceId};
use flaggo_contract_service::ContractLifecycle;
use flaggo_core_conformance::admission::{c04_c10_fenced_admission, prepare_candidate};
use flaggo_executable_store::{
    AdmissionFence, AdmissionGuard, AnalysisCandidateAdmission, AuthorityResult,
    SqliteContractAuthority,
};
use flaggo_expression::ExpressionCompiler;
use serde_json::json;

#[tokio::test]
async fn local_authority_conforms_to_fenced_admission_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspaces");
    let provider = Arc::new(LocalWorkspaceProvider::new(&root).unwrap());
    let independent_fence = Arc::new(LocalWorkspaceProvider::new(&root).unwrap());
    let path = directory.path().join("authority.db");
    let first = Arc::new(
        SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
            .await
            .unwrap()
            .with_admission_fence(provider.clone()),
    );
    first.initialize().await.unwrap();
    let second = Arc::new(
        SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
            .await
            .unwrap()
            .with_admission_fence(independent_fence),
    );
    c04_c10_fenced_admission(first.clone(), second.clone(), provider).await;
    first.close().await;
    second.close().await;
}

struct ObservedFence {
    inner: Arc<LocalWorkspaceProvider>,
    acquired: tokio::sync::Notify,
}

impl AdmissionFence for ObservedFence {
    fn acquire(
        &self,
        admission: &AnalysisCandidateAdmission,
        ownership: &CandidateOwnership,
    ) -> AuthorityResult<Box<dyn AdmissionGuard>> {
        let guard = self.inner.acquire(admission, ownership)?;
        self.acquired.notify_one();
        Ok(guard)
    }
}

#[tokio::test]
async fn cancelled_request_retains_ownership_exclusion_through_sql_commit() {
    let directory = tempfile::tempdir().unwrap();
    let provider =
        Arc::new(LocalWorkspaceProvider::new(directory.path().join("workspaces")).unwrap());
    let fence = Arc::new(ObservedFence {
        inner: provider.clone(),
        acquired: tokio::sync::Notify::new(),
    });
    let path = directory.path().join("authority.db");
    let authority = Arc::new(
        SqliteContractAuthority::open(&path, Some(Arc::new(ExpressionCompiler::default())))
            .await
            .unwrap()
            .with_admission_fence(fence.clone()),
    );
    authority.initialize().await.unwrap();
    let service = ContractLifecycle::new(authority.clone());
    let document = flaggo_core_conformance::contract();
    let resource = ContractResourceId::try_from(&document).unwrap();
    let version = service.deploy(&resource, &document).await.unwrap().0;
    let prepared = prepare_candidate(
        provider.as_ref(),
        &serde_json::to_value(version).unwrap(),
        "owner",
        json!([{"name":"learned","when":{"expression":"true"},"return":{"value":400}}]),
    )
    .await;
    let pool =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(path))
            .await
            .unwrap();
    let blocker = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let digest = prepared.contract.contract_digest.clone();
    let submission = prepared.submission.clone();
    let submitting_service = service.clone();
    let submitting_resource = resource.clone();
    let request = tokio::spawn(async move {
        submitting_service
            .submit(&submitting_resource, &digest, submission)
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), fence.acquired.notified())
        .await
        .unwrap();
    assert!(prepared.lease.release().await.is_err());
    assert!(
        prepared
            .lease
            .mark_superseding(&prepared.submission.provenance.cycle_id, None)
            .await
            .is_err()
    );
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert!(prepared.lease.release().await.is_err());
    blocker.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if prepared
                .lease
                .mark_superseding(&prepared.submission.provenance.cycle_id, None)
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let admitted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM analysis_candidate_admissions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(admitted, 1);
    assert_eq!(
        service
            .submit(
                &resource,
                &prepared.contract.contract_digest,
                prepared.submission
            )
            .await
            .unwrap_err()
            .problem()
            .status,
        409
    );
    prepared.lease.release().await.unwrap();
    authority.close().await;
    pool.close().await;
}
