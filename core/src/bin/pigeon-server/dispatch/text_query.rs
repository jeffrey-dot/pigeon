//! Negotiated bounded text queries, serialized with the pane's PTY output.

use crate::{connection::state::ClientState, host::state::HostState, pane::cmd::Cmd};
use pigeon_core::api::{PgError, TextQuery};
use pigeon_core::proto::Message;

pub async fn query(
    host: &HostState,
    state: &ClientState,
    id: u32,
    pane_id: u32,
    query: TextQuery,
) -> Message {
    if !state.text_query {
        return error(id, PgError::Unsupported);
    }
    if let Err(err) = query.validate() {
        return error(id, err);
    }
    let Some(sender) = crate::pane::query::sender_of(&host.sessions, pane_id).await else {
        return super::no_such_pane(pane_id, Some(id));
    };
    let (reply, receiver) = tokio::sync::oneshot::channel();
    if sender.send(Cmd::TextQuery { query, reply }).await.is_err() {
        return super::no_such_pane(pane_id, Some(id));
    }
    match receiver.await {
        Ok(Ok(result)) => Message::TextQueryResult {
            id,
            pane_id,
            result,
        },
        Ok(Err(err)) => error(id, err),
        Err(_) => super::no_such_pane(pane_id, Some(id)),
    }
}

fn error(id: u32, err: PgError) -> Message {
    let code = match err {
        PgError::StaleText => "stale_text",
        PgError::Limit => "limit",
        PgError::Unsupported => "unsupported",
        _ => "invalid_arg",
    };
    Message::Error {
        id: Some(id),
        code: code.into(),
        detail: err.to_string(),
    }
}
