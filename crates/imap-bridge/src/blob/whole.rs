// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! A blob read whole on a session the caller holds, for a message that
//! carries it.

use super::address::Address;
use super::{BlobError, Reader, Stop};
use crate::session::Session;
use crate::sync::Cache;

/// A blob of the account in one piece; `limit` bounds it as
/// [`super::open`] does. The session stays in hand, so its folder is
/// examined once and the caller selects what it writes to afterwards.
///
/// # Errors
///
/// As [`super::open`], beside whether the session still stands.
pub(crate) async fn whole<S: Session>(
    session: &mut S,
    cache: &Cache,
    blob_id: &str,
    limit: u64,
) -> Result<Vec<u8>, Stop> {
    let address = Address::parse(blob_id).ok_or(Stop::kept(BlobError::NotFound))?;
    let mut reader = Reader::placed(cache, &address, limit)
        .await
        .map_err(Stop::kept)?;
    reader.examined(session).await?;
    let mut part = address.part.as_deref();
    let mut bytes = Vec::new();
    loop {
        let window = reader.window(session, part.take()).await?;
        let (content, last) = reader.took(window).await.map_err(Stop::kept)?;
        bytes.extend_from_slice(&content);
        if last {
            return Ok(bytes);
        }
    }
}
