use std::time::Duration;

use reqwest::{
    Client, StatusCode, Url,
    header::{ETAG, IF_NONE_MATCH},
    redirect::Policy,
};
use thiserror::Error;

use crate::{CompiledSelectorSnapshot, SelectorSnapshotError};

const MAXIMUM_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

pub struct HttpSelectorSnapshotProvider {
    client: Client,
    endpoint: Url,
    bearer_token: Option<String>,
}

impl HttpSelectorSnapshotProvider {
    pub fn new(
        endpoint: &str,
        bearer_token: Option<String>,
    ) -> Result<Self, SnapshotProviderError> {
        let endpoint = Url::parse(endpoint)
            .map_err(|error| SnapshotProviderError::InvalidUrl(error.to_string()))?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            return Err(SnapshotProviderError::UnsupportedScheme(
                endpoint.scheme().to_owned(),
            ));
        }
        if bearer_token.as_ref().is_some_and(String::is_empty) {
            return Err(SnapshotProviderError::EmptyBearerToken);
        }
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .redirect(Policy::none())
                .build()
                .map_err(SnapshotProviderError::Client)?,
            endpoint,
            bearer_token,
        })
    }

    pub async fn fetch(
        &self,
        current_snapshot_digest: &str,
    ) -> Result<SnapshotFetch, SnapshotProviderError> {
        let mut request = self
            .client
            .get(self.endpoint.clone())
            .header(IF_NONE_MATCH, format!("\"{current_snapshot_digest}\""));
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token);
        }
        let mut response = request
            .send()
            .await
            .map_err(SnapshotProviderError::Request)?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(SnapshotFetch::NotModified);
        }
        if response.status() != StatusCode::OK {
            return Err(SnapshotProviderError::UnexpectedStatus(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAXIMUM_SNAPSHOT_BYTES as u64)
        {
            return Err(SnapshotProviderError::TooLarge);
        }
        let etag = response
            .headers()
            .get(ETAG)
            .ok_or(SnapshotProviderError::MissingEtag)?
            .to_str()
            .map_err(|_| SnapshotProviderError::InvalidEtag)?
            .to_owned();
        let mut payload =
            Vec::with_capacity(response.content_length().unwrap_or_default() as usize);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(SnapshotProviderError::Request)?
        {
            if payload.len().saturating_add(chunk.len()) > MAXIMUM_SNAPSHOT_BYTES {
                return Err(SnapshotProviderError::TooLarge);
            }
            payload.extend_from_slice(&chunk);
        }
        let snapshot = CompiledSelectorSnapshot::compile(payload)?;
        let expected_etag = format!("\"{}\"", snapshot.snapshot_digest);
        if etag != expected_etag {
            return Err(SnapshotProviderError::EtagMismatch {
                expected: expected_etag,
                actual: etag,
            });
        }
        Ok(SnapshotFetch::Updated(snapshot))
    }
}

pub enum SnapshotFetch {
    NotModified,
    Updated(CompiledSelectorSnapshot),
}

#[derive(Debug, Error)]
pub enum SnapshotProviderError {
    #[error("contract snapshot URL is invalid: {0}")]
    InvalidUrl(String),
    #[error("contract snapshot URL scheme '{0}' is unsupported")]
    UnsupportedScheme(String),
    #[error("contract snapshot bearer token must not be empty")]
    EmptyBearerToken,
    #[error("contract snapshot HTTP client could not be created: {0}")]
    Client(#[source] reqwest::Error),
    #[error("contract snapshot request failed: {0}")]
    Request(#[source] reqwest::Error),
    #[error("contract snapshot endpoint returned unexpected HTTP status {0}")]
    UnexpectedStatus(StatusCode),
    #[error("contract snapshot response is missing ETag")]
    MissingEtag,
    #[error("contract snapshot ETag is invalid")]
    InvalidEtag,
    #[error("contract snapshot ETag mismatch: expected {expected}, received {actual}")]
    EtagMismatch { expected: String, actual: String },
    #[error("contract snapshot exceeds the 16 MiB response limit")]
    TooLarge,
    #[error(transparent)]
    InvalidSnapshot(#[from] SelectorSnapshotError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Router,
        body::Body,
        extract::State,
        http::{HeaderMap, HeaderValue, StatusCode, header::ETAG},
        response::{IntoResponse, Response},
        routing::get,
    };
    use tokio::net::TcpListener;

    use super::{
        HttpSelectorSnapshotProvider, MAXIMUM_SNAPSHOT_BYTES, SnapshotFetch, SnapshotProviderError,
    };
    use crate::CompiledSelectorSnapshot;

    #[derive(Clone)]
    struct SnapshotState {
        payload: Arc<Vec<u8>>,
        etag: Arc<String>,
    }

    #[tokio::test]
    async fn conditionally_fetches_and_validates_snapshot_etags() {
        let snapshot = CompiledSelectorSnapshot::built_in_only();
        let state = SnapshotState {
            payload: Arc::new(snapshot.payload.clone()),
            etag: Arc::new(format!("\"{}\"", snapshot.snapshot_digest)),
        };
        let router = Router::new()
            .route("/snapshot", get(snapshot_response))
            .route("/oversized", get(oversized_response))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let provider =
            HttpSelectorSnapshotProvider::new(&format!("http://{address}/snapshot"), None).unwrap();

        let fetched = provider
            .fetch(&format!("sha256:{}", "0".repeat(64)))
            .await
            .unwrap();
        let SnapshotFetch::Updated(fetched) = fetched else {
            panic!("first request must return the snapshot");
        };
        assert_eq!(fetched.snapshot_digest, snapshot.snapshot_digest);
        assert!(matches!(
            provider.fetch(&snapshot.snapshot_digest).await.unwrap(),
            SnapshotFetch::NotModified
        ));
        let oversized =
            HttpSelectorSnapshotProvider::new(&format!("http://{address}/oversized"), None)
                .unwrap();
        assert!(matches!(
            oversized.fetch(&snapshot.snapshot_digest).await,
            Err(SnapshotProviderError::TooLarge)
        ));

        server.abort();
    }

    async fn oversized_response() -> Response {
        let mut response = Response::new(Body::from(vec![0_u8; MAXIMUM_SNAPSHOT_BYTES + 1]));
        *response.status_mut() = StatusCode::OK;
        response
    }

    async fn snapshot_response(State(state): State<SnapshotState>, headers: HeaderMap) -> Response {
        let etag = HeaderValue::from_str(&state.etag).unwrap();
        if headers
            .get(reqwest::header::IF_NONE_MATCH)
            .is_some_and(|value| value == etag)
        {
            return (StatusCode::NOT_MODIFIED, [(ETAG, etag)]).into_response();
        }
        (
            StatusCode::OK,
            [
                (ETAG, etag),
                (
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                ),
            ],
            state.payload.as_ref().clone(),
        )
            .into_response()
    }
}
