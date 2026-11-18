use std::time::Duration;

use reqwest::{
    Client, StatusCode, Url,
    header::{ETAG, IF_NONE_MATCH},
    redirect::Policy,
};
use thiserror::Error;

use crate::{CompiledContractCatalog, ContractCatalogError};

const MAXIMUM_CATALOG_BYTES: usize = 16 * 1024 * 1024;

pub struct HttpContractCatalogProvider {
    client: Client,
    endpoint: Url,
}

impl HttpContractCatalogProvider {
    pub fn new(endpoint: &str) -> Result<Self, CatalogProviderError> {
        let endpoint = Url::parse(endpoint)
            .map_err(|error| CatalogProviderError::InvalidUrl(error.to_string()))?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            return Err(CatalogProviderError::UnsupportedScheme(
                endpoint.scheme().to_owned(),
            ));
        }
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .redirect(Policy::none())
                .build()
                .map_err(CatalogProviderError::Client)?,
            endpoint,
        })
    }

    pub async fn fetch(
        &self,
        current_etag: Option<&str>,
    ) -> Result<CatalogFetch, CatalogProviderError> {
        let mut request = self.client.get(self.endpoint.clone());
        if let Some(etag) = current_etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let mut response = request
            .send()
            .await
            .map_err(CatalogProviderError::Request)?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return if current_etag.is_some() {
                Ok(CatalogFetch::NotModified)
            } else {
                Err(CatalogProviderError::UnexpectedNotModified)
            };
        }
        if response.status() != StatusCode::OK {
            return Err(CatalogProviderError::UnexpectedStatus(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAXIMUM_CATALOG_BYTES as u64)
        {
            return Err(CatalogProviderError::TooLarge);
        }
        let etag = response
            .headers()
            .get(ETAG)
            .ok_or(CatalogProviderError::MissingEtag)?
            .to_str()
            .map_err(|_| CatalogProviderError::InvalidEtag)?
            .to_owned();
        if etag.is_empty() {
            return Err(CatalogProviderError::InvalidEtag);
        }
        let mut payload =
            Vec::with_capacity(response.content_length().unwrap_or_default() as usize);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(CatalogProviderError::Request)?
        {
            if payload.len().saturating_add(chunk.len()) > MAXIMUM_CATALOG_BYTES {
                return Err(CatalogProviderError::TooLarge);
            }
            payload.extend_from_slice(&chunk);
        }
        Ok(CatalogFetch::Updated {
            catalog: CompiledContractCatalog::compile(payload)?,
            etag,
        })
    }
}

pub enum CatalogFetch {
    NotModified,
    Updated {
        catalog: CompiledContractCatalog,
        etag: String,
    },
}

#[derive(Debug, Error)]
pub enum CatalogProviderError {
    #[error("contract catalog URL is invalid: {0}")]
    InvalidUrl(String),
    #[error("contract catalog URL scheme '{0}' is unsupported")]
    UnsupportedScheme(String),
    #[error("contract catalog HTTP client could not be created: {0}")]
    Client(#[source] reqwest::Error),
    #[error("contract catalog request failed: {0}")]
    Request(#[source] reqwest::Error),
    #[error("contract catalog endpoint returned unexpected HTTP status {0}")]
    UnexpectedStatus(StatusCode),
    #[error("contract catalog endpoint returned 304 without a conditional ETag")]
    UnexpectedNotModified,
    #[error("contract catalog response is missing ETag")]
    MissingEtag,
    #[error("contract catalog ETag is invalid")]
    InvalidEtag,
    #[error("contract catalog exceeds the 16 MiB response limit")]
    TooLarge,
    #[error(transparent)]
    InvalidCatalog(#[from] ContractCatalogError),
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

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
        CatalogFetch, CatalogProviderError, HttpContractCatalogProvider, MAXIMUM_CATALOG_BYTES,
    };

    #[derive(Clone)]
    struct CatalogState {
        payload: Arc<Vec<u8>>,
        observed_if_none_match: Arc<Mutex<Option<String>>>,
    }

    #[tokio::test]
    async fn treats_etag_as_opaque_conditional_fetch_state() {
        let state = CatalogState {
            payload: Arc::new(br#"{"contracts":[]}"#.to_vec()),
            observed_if_none_match: Arc::new(Mutex::new(None)),
        };
        let observed = state.observed_if_none_match.clone();
        let router = Router::new()
            .route("/catalog", get(catalog_response))
            .route("/oversized", get(oversized_response))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let provider =
            HttpContractCatalogProvider::new(&format!("http://{address}/catalog")).unwrap();

        let CatalogFetch::Updated { etag, .. } = provider.fetch(None).await.unwrap() else {
            panic!("first request must return the catalog");
        };
        assert_eq!(etag, "\"opaque-catalog-tag\"");
        assert!(matches!(
            provider.fetch(Some(&etag)).await.unwrap(),
            CatalogFetch::NotModified
        ));
        assert_eq!(
            observed.lock().expect("observed header").as_deref(),
            Some("\"opaque-catalog-tag\"")
        );

        let oversized =
            HttpContractCatalogProvider::new(&format!("http://{address}/oversized")).unwrap();
        assert!(matches!(
            oversized.fetch(None).await,
            Err(CatalogProviderError::TooLarge)
        ));
        server.abort();
    }

    async fn oversized_response() -> Response {
        let mut response = Response::new(Body::from(vec![0_u8; MAXIMUM_CATALOG_BYTES + 1]));
        *response.status_mut() = StatusCode::OK;
        response
    }

    async fn catalog_response(State(state): State<CatalogState>, headers: HeaderMap) -> Response {
        let etag = HeaderValue::from_static("\"opaque-catalog-tag\"");
        let conditional = headers
            .get(reqwest::header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        *state
            .observed_if_none_match
            .lock()
            .expect("observed header") = conditional;
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
