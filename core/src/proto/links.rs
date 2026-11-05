//! Bounds and validation for the optional, frame-local OSC 8 URI table.
use super::Message;
use crate::{
    error::{PgError, PgResult},
    term::hyperlink::{valid_uri, MAX_LINKS, MAX_LINK_BYTES},
};
use std::collections::BTreeSet;

pub(super) fn validate(message: &Message) -> PgResult<()> {
    let links = match message {
        Message::Snapshot { links, .. } | Message::Delta { links, .. } => links,
        _ => return Ok(()),
    };
    let Some(links) = links else { return Ok(()) };
    if links.len() > MAX_LINKS {
        return Err(PgError::InvalidArg);
    }
    let mut ids = BTreeSet::new();
    let mut bytes = 0usize;
    for link in links {
        bytes = bytes
            .checked_add(link.uri.len())
            .ok_or(PgError::InvalidArg)?;
        if link.id == 0 || !valid_uri(&link.uri) || !ids.insert(link.id) || bytes > MAX_LINK_BYTES {
            return Err(PgError::InvalidArg);
        }
    }
    Ok(())
}
