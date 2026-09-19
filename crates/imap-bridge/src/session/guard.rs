// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A guard between the socket and the client library. The protocol
//! parser under that library recurses once per open parenthesis and the
//! library buffers a response whole. It also spends many times the
//! bytes of a response on the heap, its literals aside. So this reader
//! fails a response that passes a bound before the parser sees it.

mod lexer;
#[cfg(test)]
mod tests;

use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use lexer::Lexer;
pub use lexer::{MAX_NESTING, MAX_RESPONSE_BYTES, MAX_STRUCTURED_BYTES};

/// The bound a response passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Limit {
    Nesting,
    Size,
    Structure,
    Literal,
}

impl Limit {
    /// The fixed words a session error carries.
    pub(crate) fn words(self) -> &'static str {
        match self {
            Self::Nesting => "the answer nests too deep",
            Self::Size => "the answer passes the byte limit",
            Self::Structure => "the answer passes the structure limit",
            Self::Literal => "the answer holds a literal in free text",
        }
    }
}

impl fmt::Display for Limit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.words())
    }
}

impl std::error::Error for Limit {}

impl From<Limit> for io::Error {
    fn from(limit: Limit) -> Self {
        Self::new(io::ErrorKind::InvalidData, limit)
    }
}

/// A stream whose incoming bytes pass the lexer before the client
/// library reads them. Once a bound is passed every later read fails
/// the same way.
#[derive(Debug)]
pub(super) struct Guarded<T> {
    inner: T,
    lexer: Lexer,
    tripped: Option<Limit>,
}

impl<T> Guarded<T> {
    pub(super) fn new(inner: T) -> Self {
        Self {
            inner,
            lexer: Lexer::default(),
            tripped: None,
        }
    }

    /// The stream underneath, for the TLS handshake after STARTTLS.
    pub(super) fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Guarded<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Some(limit) = this.tripped {
            return Poll::Ready(Err(limit.into()));
        }
        let before = buf.filled().len();
        ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        if let Err(limit) = this.lexer.feed(&buf.filled()[before..]) {
            this.tripped = Some(limit);
            // The parser walks a line while it arrives, so the chunk never reaches it.
            buf.set_filled(before);
            return Poll::Ready(Err(limit.into()));
        }
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Guarded<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}
