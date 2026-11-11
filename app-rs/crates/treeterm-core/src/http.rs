//! HTTPS requests from the app process (the renderer's `fetch`), behind [`HttpClient`] so
//! clients built on it can be driven by fakes. [`ReqwestHttp`] is the real one: reqwest on
//! rustls with the platform's trust store, run on the core runtime.

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;

use crate::rt::Rt;

/// Sent with every request: GitHub refuses requests without one (`fetch` sends Chromium's).
pub const USER_AGENT: &str = "treeterm";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Empty,
    /// Sent as is; the caller sets `Content-Type`.
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A response of any status (as `fetch` resolves for 4xx/5xx too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// The reason phrase (`statusText`); the canonical one for the status, since HTTP/2
    /// carries none.
    pub status_text: String,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// `Response.ok`.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// The request did not produce a response (`fetch` rejected).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct HttpError(pub String);

/// Sends requests. Dropping the future cancels the request.
pub trait HttpClient: Send + Sync {
    fn send(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, HttpError>>;
}

/// reqwest on the core runtime. Cheap to clone; clones share the connection pool.
#[derive(Clone)]
pub struct ReqwestHttp {
    rt: Rt,
    client: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpSetupError {
    #[error("cannot set up TLS: {0}")]
    Tls(String),
    #[error("cannot build the HTTP client: {0}")]
    Client(#[from] reqwest::Error),
}

impl ReqwestHttp {
    pub fn new(rt: Rt) -> Result<Self, HttpSetupError> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .tls_backend_preconfigured(tls_config()?)
            .build()?;
        Ok(Self { rt, client })
    }
}

/// rustls on ring with the platform verifier (the system trust store), offering h2 and
/// http/1.1. Every reqwest client in the app is built with it.
pub fn tls_config() -> Result<rustls::ClientConfig, HttpSetupError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
        .map_err(|error| HttpSetupError::Tls(error.to_string()))?;
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| HttpSetupError::Tls(error.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(tls)
}

impl HttpClient for ReqwestHttp {
    fn send(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, HttpError>> {
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
        };
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Body::Text(body) = request.body {
            builder = builder.body(body);
        }
        self.rt
            .run(async move {
                let response = builder
                    .send()
                    .await
                    .map_err(|error| HttpError(error.to_string()))?;
                let status = response.status();
                let body = response
                    .bytes()
                    .await
                    .map_err(|error| HttpError(error.to_string()))?;
                Ok(HttpResponse {
                    status: status.as_u16(),
                    status_text: status.canonical_reason().unwrap_or_default().to_owned(),
                    body: body.to_vec(),
                })
            })
            .map(|result| result.unwrap_or_else(|lost| Err(HttpError(lost.to_string()))))
            .boxed()
    }
}

/// Percent-encode a query value (everything but RFC 3986 unreserved characters).
pub fn encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte))
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(
            encode_query_value("owner:feat/x+y #1"),
            "owner%3Afeat%2Fx%2By%20%231"
        );
        assert_eq!(encode_query_value("main"), "main");
    }

    #[test]
    fn ok_is_any_2xx() {
        let response = |status| HttpResponse {
            status,
            status_text: String::new(),
            body: Vec::new(),
        };
        assert!(response(200).ok());
        assert!(response(201).ok());
        assert!(!response(304).ok());
        assert!(!response(403).ok());
    }

    #[test]
    fn the_real_client_builds_inside_a_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let rt = Rt::new(runtime.handle().clone());
        let _guard = runtime.enter();
        assert!(ReqwestHttp::new(rt).is_ok());
    }
}
