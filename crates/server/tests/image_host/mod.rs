// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! An image host for the remote-image proxy tests: a PNG, pages that
//! are no image, redirects of every shape, bodies past the bound, a
//! request parked until the test releases it and every request's
//! headers on record.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use futures_util::{StreamExt as _, stream};
use huliho_server::mail::remote::{REMOTE_IMAGE_LIMIT, REMOTE_REDIRECTS};
use tokio::sync::Notify;

use crate::tls_server::TlsServer;

/// The name the host lives under; the test certificate carries it.
pub const HOST: &str = "example.test";
/// A second name under the certificate, for a redirect across hosts.
pub const OTHER_HOST: &str = "other.test";
/// A name under the certificate that the resolver answers with a
/// private address.
pub const INWARD_HOST: &str = "inside.example.test";

/// The bytes of a small PNG: the signature and the start of its header.
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";

/// The chunk an endless body streams over and over.
const ENDLESS_CHUNK: usize = 64 * 1024;

/// Every request so far: its path and its headers.
type Seen = Arc<Mutex<Vec<(String, HeaderMap)>>>;

pub struct ImageHost {
    pub server: TlsServer,
    seen: Seen,
    release: Arc<Notify>,
}

impl ImageHost {
    pub async fn start() -> Self {
        let seen: Seen = Arc::default();
        let release = Arc::new(Notify::new());
        let app = routes(Arc::clone(&release))
            .layer(middleware::from_fn_with_state(Arc::clone(&seen), record));
        let server = TlsServer::start(app).await;
        Self {
            server,
            seen,
            release,
        }
    }

    pub fn port(&self) -> u16 {
        self.server.address.port()
    }

    /// A URL at this host under `host`, a name the resolver answers.
    pub fn url(&self, host: &str, path: &str) -> String {
        format!("https://{host}:{}{path}", self.port())
    }

    pub fn seen(&self) -> Vec<(String, HeaderMap)> {
        self.seen.lock().unwrap().clone()
    }

    /// Lets one parked request answer its image.
    pub fn release(&self) {
        self.release.notify_one();
    }
}

/// The image once the test releases the request.
async fn parked(State(release): State<Arc<Notify>>) -> Response {
    release.notified().await;
    image()
}

async fn record(State(seen): State<Seen>, request: Request, next: Next) -> Response {
    seen.lock()
        .unwrap()
        .push((request.uri().path().to_owned(), request.headers().clone()));
    next.run(request).await
}

fn routes(release: Arc<Notify>) -> Router {
    Router::new()
        .route("/image.png", get(|| async { image() }))
        .route("/parked", get(parked))
        .route(
            "/page.html",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/html")],
                    "<html><body>x</body></html>",
                )
            }),
        )
        .route(
            "/logo.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    "<svg xmlns='http://www.w3.org/2000/svg'/>",
                )
            }),
        )
        .route(
            "/declared-large",
            get(|| async { endless(Some(REMOTE_IMAGE_LIMIT + 1)) }),
        )
        .route("/endless", get(|| async { endless(None) }))
        .route(
            "/status/{code}",
            get(|Path(code): Path<u16>| async move {
                (StatusCode::from_u16(code).unwrap(), PNG)
            }),
        )
        .route("/to/{target}", get(to))
        .route(
            "/hop/{n}",
            get(|Path(n): Path<usize>| async move {
                Redirect::temporary(&format!("/hop/{}", n + 1))
            }),
        )
        .route("/astray/{n}", get(astray))
        .route(
            "/three/{n}",
            get(|Path(n): Path<usize>| async move {
                if n >= REMOTE_REDIRECTS {
                    image()
                } else {
                    Redirect::temporary(&format!("/three/{}", n + 1)).into_response()
                }
            }),
        )
        .with_state(release)
}

fn image() -> Response {
    ([(header::CONTENT_TYPE, "image/png")], PNG).into_response()
}

/// The PNG followed by zeros without end, declared at the given length
/// or streamed without one.
fn endless(declared: Option<u64>) -> Response {
    let zeros = Bytes::from(vec![0; ENDLESS_CHUNK]);
    let first = stream::iter([Ok::<Bytes, std::io::Error>(Bytes::from_static(PNG))]);
    let body = Body::from_stream(first.chain(stream::repeat_with(move || Ok(zeros.clone()))));
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png");
    if let Some(declared) = declared {
        response = response.header(header::CONTENT_LENGTH, declared);
    }
    response.body(body).unwrap()
}

/// Redirects of every shape, on the port the Host header names.
async fn to(Path(target): Path<String>, headers: HeaderMap) -> Response {
    let host = headers[header::HOST].to_str().unwrap();
    let port = host.rsplit(':').next().unwrap();
    let location = match target.as_str() {
        "private" => format!("https://{INWARD_HOST}:{port}/image.png"),
        "http" => format!("http://{HOST}:{port}/image.png"),
        "user" => format!("https://sanne:secret@{HOST}:{port}/image.png"),
        "other" => format!("https://{OTHER_HOST}:{port}/image.png"),
        "literal" => format!("https://127.0.0.1:{port}/image.png"),
        "relative" => "/image.png".to_owned(),
        "nowhere" => return StatusCode::FOUND.into_response(),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    Redirect::temporary(&location).into_response()
}

/// A chain that points at an address literal once the redirects allowed
/// are followed.
async fn astray(Path(n): Path<usize>, headers: HeaderMap) -> Response {
    if n < REMOTE_REDIRECTS {
        Redirect::temporary(&format!("/astray/{}", n + 1)).into_response()
    } else {
        to(Path("literal".to_owned()), headers).await
    }
}
