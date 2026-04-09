use std::task::{Context, Poll};

use http::Request;
use tower::{Layer, Service};

/// Daemon-generated per-connection identifier.
/// Injected into every request's extensions by the tower layer.
#[derive(Clone, Debug)]
pub struct ConnectionId(pub String);

/// Tower layer that assigns a unique UUID to each gRPC connection.
///
/// `Layer::layer()` is called once per accepted connection, generating
/// a fresh UUID. All requests on that connection share the same ID.
#[derive(Clone)]
pub struct ConnectionIdLayer;

impl<S> Layer<S> for ConnectionIdLayer {
    type Service = ConnectionIdService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConnectionIdService {
            inner,
            connection_id: ConnectionId(uuid::Uuid::new_v4().to_string()),
        }
    }
}

/// Wraps an inner service, injecting `ConnectionId` into every request.
#[derive(Clone)]
pub struct ConnectionIdService<S> {
    inner: S,
    connection_id: ConnectionId,
}

impl<S, B> Service<Request<B>> for ConnectionIdService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        req.extensions_mut().insert(self.connection_id.clone());
        self.inner.call(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_generates_unique_ids() {
        let layer = ConnectionIdLayer;

        // Two connections get different IDs
        let svc1 = layer.layer(());
        let svc2 = layer.layer(());
        assert_ne!(svc1.connection_id.0, svc2.connection_id.0);
    }

    #[test]
    fn clone_preserves_id() {
        let layer = ConnectionIdLayer;
        let svc = layer.layer(());
        let cloned = svc.clone();
        assert_eq!(svc.connection_id.0, cloned.connection_id.0);
    }
}
