//! Text queries for embedded panes and the host protocol use the same contract.

use super::{Core, Message, PaneId, PgError, PgResult};
pub use crate::proto::text_viewport::TextViewport;
pub use crate::term::text_query::{
    TextInfo, TextPoint, TextQuery, TextRange, TextResult, MAX_QUERY_BYTES, MAX_TEXT_BYTES,
    MAX_TEXT_RESULTS,
};

impl Core {
    /// Reads one bounded page from the pane's current screen and scrollback.
    /// Search generation=0 starts fresh; subsequent search/range requests must carry
    /// the returned generation. Output or resize invalidates it with StaleText.
    /// Missing continuation means complete; stopping pagination cancels without state.
    pub fn text_query(&self, pane_id: PaneId, query: &TextQuery) -> PgResult<TextResult> {
        self.pane(pane_id)?.text_query(query)
    }
}

/// Builds a validated request; send only after negotiating the text_query capability.
pub fn text_query_message(id: u32, pane_id: PaneId, query: TextQuery) -> PgResult<Message> {
    query.validate()?;
    Ok(Message::TextQuery { id, pane_id, query })
}

/// Copies and validates a result, returning its request id and pane id.
pub fn text_result_of(message: &Message) -> PgResult<(u32, PaneId, TextResult)> {
    let Message::TextQueryResult {
        id,
        pane_id,
        result,
    } = message
    else {
        return Err(PgError::InvalidArg);
    };
    crate::proto::text_query::validate_result(result)?;
    Ok((*id, *pane_id, result.clone()))
}

/// Reads validated viewport metadata and wire cells from a location result.
pub fn text_viewport_of(result: &TextResult) -> PgResult<&TextViewport> {
    crate::proto::text_query::validate_result(result)?;
    match result {
        TextResult::Viewport { viewport, .. } => Ok(viewport),
        _ => Err(PgError::InvalidArg),
    }
}

/// The independently owned picture returned by a Viewport query. It has no history;
/// row zero corresponds to -viewport.offset in the result's generation.
pub fn text_viewport_snapshot(result: &TextResult) -> PgResult<super::Snapshot> {
    crate::proto::text_query::validate_result(result)?;
    let TextResult::Viewport { info, viewport, .. } = result else {
        return Err(PgError::InvalidArg);
    };
    crate::session::snapshot_from_message(&viewport.message(*info)).ok_or(PgError::InvalidArg)
}
