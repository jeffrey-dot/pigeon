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
        if let TextQuery::Viewport { generation, range } = query {
            query.validate()?;
            if *generation != info.generation {
                return Err(PgError::StaleText);
            }
            crate::term::valid_text_point(self.term.as_ref(), info, range.start)?;
            crate::term::valid_text_point(self.term.as_ref(), info, range.end)?;
            let offset = range.start.row.min(0).unsigned_abs();
            let (snapshot, history_len) = self.term.scrolled_snapshot(offset as usize);
            let message = super::message_from_snapshot(
                self.pane_id,
                self.seq,
                &snapshot,
                false,
                self.term.modes(),
            );
            let crate::proto::Message::Snapshot {
                rows,
                cursor,
                scroll,
                screen,
                text,
                links,
                ..
            } = message
            else {
                return Err(PgError::InvalidArg);
            };
            return Ok(TextResult::Viewport {
                info,
                range: *range,
                viewport: Box::new(crate::proto::text_viewport::TextViewport {
                    rows,
                    cursor,
                    scroll,
                    screen,
                    text,
                    links,
                    modes: self.term.modes(),
                    offset,
                    history_len: history_len as u32,
                }),
            });
        }
        crate::term::query_text(self.term.as_ref(), info, query)
    }
}
