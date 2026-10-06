use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use flaggo_analysis_domain::{
    AnalysisAuthority, AnalysisContract, AnalysisContractIdentity, AnalysisError,
    AnalysisEvidenceSource, AttemptContext, CandidateRecord, CatalogUpdate, EvidenceCutoff,
};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ETAG, IF_NONE_MATCH},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::ContractService;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct ContractServiceClient {
    client: Client,
    base_url: Url,
    catalog_url: Url,
}

impl ContractServiceClient {
    pub fn new(
        base_url: &str,
        catalog_url: Option<&str>,
        timeout: Duration,
    ) -> Result<Self, AnalysisError> {
        let base_url = Url::parse(base_url)
            .map_err(|error| AnalysisError::Contract(format!("invalid service URL: {error}")))?;
        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(AnalysisError::Contract(
                "Contract Service URL must use HTTP or HTTPS".to_owned(),
            ));
        }
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(contract_error)?;
        let catalog_url = match catalog_url {
            Some(catalog_url) => Url::parse(catalog_url).map_err(|error| {
                AnalysisError::Contract(format!("invalid contract catalog URL: {error}"))
            })?,
            None => {
                let temporary = Self {
                    client: client.clone(),
                    base_url: base_url.clone(),
                    catalog_url: base_url.clone(),
                };
                temporary.endpoint(["v3", "decision-contract-catalog", "current"])?
            }
        };
        if !matches!(catalog_url.scheme(), "http" | "https") {
            return Err(AnalysisError::Contract(
                "contract catalog URL must use HTTP or HTTPS".to_owned(),
            ));
        }
        Ok(Self {
            client,
            base_url,
            catalog_url,
        })
    }

    fn endpoint<'a>(
        &self,
        segments: impl IntoIterator<Item = &'a str>,
    ) -> Result<Url, AnalysisError> {
        let mut url = self.base_url.clone();
        let mut path = url.path_segments_mut().map_err(|()| {
            AnalysisError::Contract("Contract Service URL cannot be a base URL".to_owned())
        })?;
        path.pop_if_empty();
        path.extend(segments);
        drop(path);
        Ok(url)
    }

    async fn current_response(
        &self,
        contract_name: &str,
    ) -> Result<Option<AcceptedVersionWire>, AnalysisError> {
        let url = self.endpoint(["v3", "decision-contracts", contract_name])?;
        let response = self.client.get(url).send().await.map_err(contract_error)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure_success(response).await.map(Some)
    }
}

#[async_trait]
impl ContractService for ContractServiceClient {
    async fn list_current_learning_contracts(
        &self,
        revision: Option<&str>,
    ) -> Result<CatalogUpdate, AnalysisError> {
        let mut request = self.client.get(self.catalog_url.clone());
        if let Some(revision) = revision {
            request = request.header(IF_NONE_MATCH, revision);
        }
        let response = request.send().await.map_err(contract_error)?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(CatalogUpdate::NotModified);
        }
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }
        let revision = response
            .headers()
            .get(ETAG)
            .ok_or_else(|| {
                AnalysisError::Contract("Contract Service catalog response omitted ETag".to_owned())
            })?
            .to_str()
            .map_err(contract_error)?
            .to_owned();
        let catalog: CatalogWire = read_json_response(response).await?;
        let mut contracts = Vec::new();
        for entry in catalog.contracts {
            let projection = project_contract(&entry.contract)?;
            if projection.learning.is_some() {
                contracts.push(AnalysisContractIdentity {
                    name: projection.name,
                    digest: entry.contract_digest,
                });
            }
        }
        Ok(CatalogUpdate::Updated {
            revision,
            contracts,
        })
    }

    async fn get_current_contract(
        &self,
        contract_name: &str,
    ) -> Result<Option<AnalysisContract>, AnalysisError> {
        let Some(version) = self.current_response(contract_name).await? else {
            return Ok(None);
        };
        parse_learning_contract(version)
    }

    async fn submit_candidate(
        &self,
        context: &AttemptContext,
        rules: Value,
        cutoff: &EvidenceCutoff,
        analysis_manifest_digest: &str,
    ) -> Result<CandidateRecord, AnalysisError> {
        if !rules.is_array() {
            return Err(AnalysisError::Candidate(
                "Candidate rules must be a JSON array".to_owned(),
            ));
        }
        let evidence_watermark = i64::try_from(cutoff.watermark).map_err(|_| {
            AnalysisError::Candidate(
                "evidence watermark exceeds the Candidate API range".to_owned(),
            )
        })?;
        let url = self.endpoint([
            "v3",
            "decision-contracts",
            &context.contract.name,
            "versions",
            &context.contract.digest,
            "candidates",
        ])?;
        let response = self
            .client
            .post(url)
            .json(&CandidateSubmission {
                rules,
                provenance: CandidateProvenance {
                    workspace_id: context.workspace_id.clone(),
                    cycle_id: context.cycle_id.clone(),
                    attempt_id: context.attempt_id.clone(),
                    evidence_cutoff: cutoff.cutoff,
                    evidence_watermark,
                    analysis_manifest_digest: analysis_manifest_digest.to_owned(),
                },
            })
            .send()
            .await
            .map_err(candidate_error)?;
        if !response.status().is_success() {
            return Err(candidate_response_error(response).await);
        }
        let result: CandidateResult = read_candidate_json(response).await?;
        let valid_lifecycle = matches!(
            (result.created, result.lifecycle_state.as_str()),
            (true, "candidate") | (false, "candidate" | "active" | "inactive")
        );
        if result.contract_name != context.contract.name
            || result.contract_digest != context.contract.digest
            || !valid_lifecycle
        {
            return Err(AnalysisError::Candidate(
                "Contract Service returned an invalid Candidate identity or lifecycle".to_owned(),
            ));
        }
        Ok(CandidateRecord {
            contract_name: result.contract_name,
            contract_digest: result.contract_digest,
            executable_digest: result.executable_digest,
            created_at: result.created_at,
            created: result.created,
            analysis_manifest_digest: analysis_manifest_digest.to_owned(),
        })
    }
}

fn parse_learning_contract(
    version: AcceptedVersionWire,
) -> Result<Option<AnalysisContract>, AnalysisError> {
    let projection = project_contract(&version.contract)?;
    let Some(learning) = projection.learning else {
        return Ok(None);
    };
    if learning.evidence.is_empty() {
        return Err(AnalysisError::Contract(
            "learning contract contained no evidence declarations".to_owned(),
        ));
    }

    let mut evidence_sources = Vec::with_capacity(learning.evidence.len());
    for evidence in learning.evidence {
        let source = evidence.source;
        evidence_sources.push(AnalysisEvidenceSource {
            kind: source.kind,
            scope: source.scope,
            name: source.name,
            metric_kind: source.metric_kind,
            unit: source.unit,
            span_name: source.span_name,
        });
    }
    Ok(Some(AnalysisContract {
        name: projection.name,
        contract_digest: version.contract_digest,
        accepted_at: version.accepted_at,
        authority: AnalysisAuthority {
            tenant: projection.authority.tenant,
            application: projection.authority.application,
            environment: projection.authority.environment,
        },
        evaluation_interval: learning.policy.evaluate.interval,
        evidence_sources,
        contract: version.contract,
    }))
}

fn project_contract(contract: &Value) -> Result<ContractProjection, AnalysisError> {
    serde_json::from_value(contract.clone()).map_err(|error| {
        AnalysisError::Contract(format!(
            "invalid Contract Service contract payload: {error}"
        ))
    })
}

async fn ensure_success<T: DeserializeOwned>(response: Response) -> Result<T, AnalysisError> {
    if response.status().is_success() {
        read_json_response(response).await
    } else {
        Err(response_error(response).await)
    }
}

async fn read_json_response<T: DeserializeOwned>(response: Response) -> Result<T, AnalysisError> {
    let bytes = bounded_response_bytes(response)
        .await
        .map_err(contract_error)?;
    serde_json::from_slice(&bytes).map_err(contract_error)
}

async fn read_candidate_json<T: DeserializeOwned>(response: Response) -> Result<T, AnalysisError> {
    let bytes = bounded_response_bytes(response)
        .await
        .map_err(candidate_error)?;
    serde_json::from_slice(&bytes).map_err(candidate_error)
}

async fn bounded_response_bytes(response: Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("service response exceeded the configured size limit".to_owned());
    }
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("service response exceeded the configured size limit".to_owned());
    }
    Ok(bytes.to_vec())
}

async fn response_error(response: Response) -> AnalysisError {
    let status = response.status();
    let detail = bounded_error_detail(response).await;
    AnalysisError::Contract(format!("Contract Service returned HTTP {status}: {detail}"))
}

async fn candidate_response_error(response: Response) -> AnalysisError {
    let status = response.status();
    let detail = bounded_error_detail(response).await;
    AnalysisError::Candidate(format!(
        "Contract Service Candidate admission returned HTTP {status}: {detail}"
    ))
}

async fn bounded_error_detail(response: Response) -> String {
    match response.bytes().await {
        Ok(bytes) => {
            let length = bytes.len().min(4096);
            String::from_utf8_lossy(&bytes[..length]).into_owned()
        }
        Err(error) => format!("response body unavailable: {error}"),
    }
}

fn contract_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Contract(error.to_string())
}

fn candidate_error(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::Candidate(error.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogWire {
    contracts: Vec<CatalogEntryWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntryWire {
    contract_digest: String,
    contract: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedVersionWire {
    contract_digest: String,
    accepted_at: DateTime<Utc>,
    contract: Value,
}

#[derive(Deserialize)]
struct ContractProjection {
    authority: AuthorityWire,
    name: String,
    learning: Option<LearningWire>,
}

#[derive(Deserialize)]
struct AuthorityWire {
    tenant: String,
    application: String,
    environment: String,
}

#[derive(Deserialize)]
struct LearningWire {
    policy: LearningPolicyWire,
    evidence: Vec<EvidenceWire>,
}

#[derive(Deserialize)]
struct LearningPolicyWire {
    evaluate: LearningEvaluateWire,
}

#[derive(Deserialize)]
struct LearningEvaluateWire {
    interval: String,
}

#[derive(Deserialize)]
struct EvidenceWire {
    source: EvidenceSourceWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceSourceWire {
    kind: String,
    scope: String,
    name: String,
    metric_kind: Option<String>,
    unit: Option<String>,
    span_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidateSubmission {
    rules: Value,
    provenance: CandidateProvenance,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidateProvenance {
    workspace_id: String,
    cycle_id: String,
    attempt_id: String,
    evidence_cutoff: DateTime<Utc>,
    evidence_watermark: i64,
    analysis_manifest_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CandidateResult {
    contract_name: String,
    contract_digest: String,
    executable_digest: String,
    lifecycle_state: String,
    created_at: DateTime<Utc>,
    created: bool,
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use axum::{
        Json, Router,
        body::Body,
        extract::{Path, State},
        http::{HeaderMap, Response, StatusCode, header::CONTENT_TYPE},
        routing::{get, post},
    };
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::*;

    #[derive(Default)]
    struct TestState {
        submissions: Mutex<Vec<Value>>,
        candidate_resolved: AtomicBool,
    }

    fn learning_contract() -> Value {
        json!({
            "authority": {
                "tenant": "local",
                "application": "checkout",
                "environment": "test"
            },
            "name": "checkout.delay",
            "expression_syntax": "flaggo.cel/v1",
            "attributes": [],
            "result": {"schema": {"type": "integer"}},
            "learning": {
                "policy": {
                    "mode": "auto-activation",
                    "evaluate": {"interval": "PT1H"}
                },
                "evidence": [{
                    "name": "latency",
                    "attribute": "region",
                    "correlateBy": [],
                    "source": {
                        "kind": "metric",
                        "scope": "checkout",
                        "name": "request.duration",
                        "metricKind": "histogram",
                        "unit": "ms",
                        "correlation": {}
                    }
                }],
                "objective": {
                    "primary": {"evidence": "latency", "direction": "minimize"}
                }
            }
        })
    }

    async fn catalog(headers: HeaderMap) -> Response<Body> {
        if headers
            .get(IF_NONE_MATCH)
            .is_some_and(|value| value == "\"revision-1\"")
        {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .body(Body::empty())
                .expect("304 response");
        }
        let payload = json!({
            "contracts": [{
                "contractDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "contract": learning_contract()
            }, {
                "contractDigest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "contract": {
                    "authority": {
                        "tenant": "local",
                        "application": "checkout",
                        "environment": "test"
                    },
                    "name": "checkout.static"
                }
            }]
        });
        Response::builder()
            .status(StatusCode::OK)
            .header(ETAG, "\"revision-1\"")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .expect("catalog response")
    }

    async fn current(Path(contract_name): Path<String>) -> Response<Body> {
        if contract_name != "checkout.delay" {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .expect("not found response");
        }
        let payload = json!({
            "contractDigest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "acceptedAt": "2026-03-01T00:00:00Z",
            "contract": learning_contract()
        });
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .expect("current response")
    }

    async fn candidate(
        State(state): State<Arc<TestState>>,
        Path((contract_name, contract_digest)): Path<(String, String)>,
        Json(submission): Json<Value>,
    ) -> Response<Body> {
        state
            .submissions
            .lock()
            .expect("submission lock")
            .push(submission);
        let resolved = state.candidate_resolved.load(Ordering::SeqCst);
        let payload = json!({
            "contractName": contract_name,
            "contractDigest": contract_digest,
            "executableDigest": "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            "lifecycleState": if resolved { "active" } else { "candidate" },
            "createdAt": "2026-03-01T02:00:00Z",
            "created": !resolved
        });
        Response::builder()
            .status(if resolved {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            })
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(payload.to_string()))
            .expect("Candidate response")
    }

    async fn start_server() -> (String, Arc<TestState>, tokio::task::JoinHandle<()>) {
        let state = Arc::new(TestState::default());
        let app = Router::new()
            .route("/v3/decision-contract-catalog/current", get(catalog))
            .route("/v3/decision-contracts/{contract_name}", get(current))
            .route(
                "/v3/decision-contracts/{contract_name}/versions/{contract_digest}/candidates",
                post(candidate),
            )
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve test requests");
        });
        (format!("http://{address}"), state, server)
    }

    #[tokio::test]
    async fn reads_learning_catalog_and_current_contract_with_etag() {
        let (base_url, _state, server) = start_server().await;
        let client =
            ContractServiceClient::new(&base_url, None, Duration::from_secs(2)).expect("client");

        let update = client
            .list_current_learning_contracts(None)
            .await
            .expect("catalog");
        assert_eq!(
            update,
            CatalogUpdate::Updated {
                revision: "\"revision-1\"".to_owned(),
                contracts: vec![AnalysisContractIdentity {
                    name: "checkout.delay".to_owned(),
                    digest:
                        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .to_owned(),
                }]
            }
        );
        assert_eq!(
            client
                .list_current_learning_contracts(Some("\"revision-1\""))
                .await
                .expect("conditional catalog"),
            CatalogUpdate::NotModified
        );

        let contract = client
            .get_current_contract("checkout.delay")
            .await
            .expect("current contract")
            .expect("learning contract");
        assert_eq!(contract.name, "checkout.delay");
        assert_eq!(contract.evaluation_interval, "PT1H");
        assert_eq!(
            contract.evidence_sources[0].metric_kind.as_deref(),
            Some("histogram")
        );
        assert!(
            client
                .get_current_contract("missing")
                .await
                .expect("missing current")
                .is_none()
        );
        server.abort();
    }

    #[tokio::test]
    async fn submits_trusted_candidate_provenance() {
        let (base_url, state, server) = start_server().await;
        let client =
            ContractServiceClient::new(&base_url, None, Duration::from_secs(2)).expect("client");
        let cutoff = Utc
            .with_ymd_and_hms(2026, 3, 1, 1, 30, 0)
            .single()
            .expect("valid timestamp");
        let context = AttemptContext {
            workspace_id: "sha256:wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww"
                .to_owned(),
            cycle_id: "cycle-1".to_owned(),
            attempt_id: "attempt-1".to_owned(),
            contract: AnalysisContractIdentity {
                name: "checkout.delay".to_owned(),
                digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_owned(),
            },
        };
        let candidate = client
            .submit_candidate(
                &context,
                json!([{
                    "name": "learned",
                    "when": {"expression": "true"},
                    "return": {"value": 5}
                }]),
                &EvidenceCutoff {
                    cutoff,
                    watermark: 42,
                },
                "sha256:mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm",
            )
            .await
            .expect("submit Candidate");
        assert!(candidate.created);
        assert_eq!(
            candidate.executable_digest,
            "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        );

        state.candidate_resolved.store(true, Ordering::SeqCst);
        let retry = client
            .submit_candidate(
                &context,
                json!([{
                    "name": "learned",
                    "when": {"expression": "true"},
                    "return": {"value": 5}
                }]),
                &EvidenceCutoff {
                    cutoff,
                    watermark: 42,
                },
                "sha256:mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm",
            )
            .await
            .expect("recover resolved Candidate admission");
        assert!(!retry.created);
        assert_eq!(retry.executable_digest, candidate.executable_digest);

        let submissions = state.submissions.lock().expect("submission lock");
        assert_eq!(submissions.len(), 2);
        assert_eq!(
            submissions[0]["provenance"]["workspaceId"],
            context.workspace_id
        );
        assert_eq!(submissions[0]["provenance"]["cycleId"], "cycle-1");
        assert_eq!(submissions[0]["provenance"]["evidenceWatermark"], 42);
        server.abort();
    }
}
