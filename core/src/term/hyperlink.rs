//! OSC 8 metadata (https://github.com/kovidgoyal/kitty/blob/master/docs/hyperlinks.rst).
//! IDs are pane-local, monotonically allocated and never reused, including after RIS.

use alacritty_terminal::term::cell::Cell as VtCell;
use std::{collections::BTreeMap, sync::Arc};

pub const MAX_URI_BYTES: usize = 2048;
pub const MAX_LINKS: usize = 1024;
pub const MAX_LINK_BYTES: usize = 256 * 1024;

pub fn valid_uri(uri: &str) -> bool {
    !uri.is_empty()
        && uri.len() <= MAX_URI_BYTES
        && uri.bytes().filter(|b| *b == b';').count() <= 13
        && !uri.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

/// A bounded export, shared by every row of one snapshot/delta. Excess links become text.
#[derive(Default)]
pub(super) struct LinkExport {
    entries: BTreeMap<u32, Arc<str>>,
    bytes: usize,
}

impl LinkExport {
    pub(super) fn cell(&mut self, cell: &VtCell) -> (u32, Option<Arc<str>>) {
        let Some(link) = cell.hyperlink() else {
            return (0, None);
        };
        let Some(id) = link
            .id()
            .strip_prefix("pigeon-")
            .and_then(|id| id.parse::<u32>().ok())
        else {
            return (0, None);
        };
        if let Some(uri) = self.entries.get(&id) {
            return (id, Some(Arc::clone(uri)));
        }
        if !valid_uri(link.uri())
            || self.entries.len() >= MAX_LINKS
            || self.bytes + link.uri().len() > MAX_LINK_BYTES
        {
            return (0, None);
        }
        let uri: Arc<str> = Arc::from(link.uri());
        self.bytes += uri.len();
        self.entries.insert(id, Arc::clone(&uri));
        (id, Some(uri))
    }
}
