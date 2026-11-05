//! Queries share the pane actor's ownership; generation changes before pending damage is sent.

use super::PaneState;
use crate::error::{PgError, PgResult};
use crate::term::text_query::{TextQuery, TextResult};

impl PaneState {
    pub fn text_query(&self, query: &TextQuery) -> PgResult<TextResult> {
        if self.text_generation == u64::MAX {
            return Err(PgError::Limit);
        }
        let mut info = self.term.text_info()?;
        info.generation = self.text_generation;
        crate::term::query_text(self.term.as_ref(), info, query)
    }
}
