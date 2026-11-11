//! Per-connection identity for gRPC requests.
//!
//! Session locks belong to a connection (a client keeps lock-sensitive calls on one HTTP/2
//! connection of their own), so every request must carry the id of the connection it came
//! in on. The id is minted when the socket is accepted and handed to tonic as the stream's
//! connect info, which tonic inserts into each request's extensions.
//!
//! (A tower layer on the server cannot do this: tonic builds the layered service once and
//! clones it for every connection, so an id minted in the layer is shared by all of them.)

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::UnixStream;
use tonic::transport::server::Connected;

/// Daemon-generated per-connection identifier, found in every request's extensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionId(pub String);

/// An accepted socket together with its connection id.
pub struct IdentifiedStream {
    inner: UnixStream,
    id: ConnectionId,
}

impl IdentifiedStream {
    pub fn new(inner: UnixStream) -> Self {
        Self {
            inner,
            id: ConnectionId(uuid::Uuid::new_v4().to_string()),
        }
    }
}

impl Connected for IdentifiedStream {
    type ConnectInfo = ConnectionId;

    fn connect_info(&self) -> ConnectionId {
        self.id.clone()
    }
}

impl AsyncRead for IdentifiedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for IdentifiedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn each_accepted_stream_gets_its_own_id() {
        let (a, _) = UnixStream::pair().unwrap();
        let (b, _) = UnixStream::pair().unwrap();
        let a = IdentifiedStream::new(a);
        let b = IdentifiedStream::new(b);
        assert_ne!(a.connect_info(), b.connect_info());
    }

    #[tokio::test]
    async fn a_stream_keeps_its_id() {
        let (a, _) = UnixStream::pair().unwrap();
        let a = IdentifiedStream::new(a);
        assert_eq!(a.connect_info(), a.connect_info());
    }
}
