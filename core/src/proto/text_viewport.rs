//! One bounded, immutable picture for a generation-bound text range.
use super::{Cell, CellText, Cursor, Link, Message, Scroll};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextViewport {
    pub rows: u16,
    pub cursor: Cursor,
    pub scroll: Scroll,
    pub screen: Vec<Vec<Cell>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<CellText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<Link>>,
    pub modes: u32,
    pub offset: u32,
    pub history_len: u32,
}

impl TextViewport {
    pub(crate) fn message(&self, info: crate::term::text_query::TextInfo) -> Message {
        Message::Snapshot {
            pane_id: 0,
            seq: 0,
            cols: info.cols,
            rows: self.rows,
            alt: info.alt,
            cursor: self.cursor,
            scroll: self.scroll,
            screen: self.screen.clone(),
            text: self.text.clone(),
            links: self.links.clone(),
            history: None,
            modes: Some(self.modes),
            offset: Some(self.offset),
            history_len: Some(self.history_len),
        }
    }
}
