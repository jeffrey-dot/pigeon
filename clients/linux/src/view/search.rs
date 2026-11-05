//! Search routing stays on the current connection/selection; pictures come from core.
use super::Workspace;
use crate::search::Search;
use gtk4::prelude::*;
use pigeon_core::api::{self, Message, TextResult};

impl Workspace {
    pub fn open_search(&mut self) -> bool {
        let Some(pane) = self.focus() else {
            return false;
        };
        if !self.model.connected {
            return false;
        }
        if self
            .search
            .as_ref()
            .is_some_and(|s| s.pane == pane && s.selection == self.model.selection_generation)
        {
            return true;
        }
        self.close_search(false);
        self.return_live(pane);
        self.search = Some(Search::new(pane, self.model.selection_generation));
        if !self.search_supported {
            if let Some(search) = self.search.as_mut() {
                search.notice = "This host does not support search with location".into();
            }
        }
        true
    }

    pub fn begin_search(&mut self, pattern: String, sensitive: bool) {
        if !self.search_supported {
            return;
        }
        if let Some(search) = self.search.as_mut() {
            search.begin(pattern, sensitive);
            if let Some(screen) = self.model.panes.get_mut(&search.pane) {
                screen.search_highlight.clear();
            }
        }
        self.area.queue_render();
        self.search_page();
    }

    pub fn search_page(&mut self) {
        if !self.search.as_ref().is_some_and(Search::needs_page) {
            return;
        }
        let id = self.model.request_id();
        if let Some(search) = self.search.as_mut() {
            if let Some(query) = search.page(id) {
                if let Ok(message) = api::text_query_message(id, search.pane, query) {
                    self.host.send(message);
                }
            }
        }
    }

    pub fn navigate_search(&mut self, previous: bool) {
        let id = self.model.request_id();
        if let Some(search) = self.search.as_mut() {
            if let Some(query) = search.navigate(previous, id) {
                if let Ok(message) = api::text_query_message(id, search.pane, query) {
                    self.host.send(message);
                }
            }
        }
    }

    pub fn close_search(&mut self, restore: bool) {
        let Some(search) = self.search.take() else {
            return;
        };
        if let Some(screen) = self.model.panes.get_mut(&search.pane) {
            screen.search_highlight.clear();
            if restore && screen.search_picture.is_some() {
                screen.search_returning = true;
                screen.scroll = Default::default();
                let effects = self.model.attach(search.pane);
                self.send_effects(effects);
            } else {
                screen.search_picture = None;
                screen.search_returning = false;
            }
        }
        self.area.queue_render();
    }

    pub(super) fn search_message(&mut self, message: &Message) -> bool {
        if self.search.as_ref().is_some_and(|s| {
            s.selection != self.model.selection_generation
                || self.model.focus != Some(s.pane)
                || !self.model.connected
        }) {
            self.close_search(false);
            return false;
        }
        let Some(search) = self.search.as_mut() else {
            return false;
        };
        match message {
            Message::Error {
                id: Some(id), code, ..
            } => search.fail(*id, code),
            Message::Snapshot { pane_id, .. } | Message::Delta { pane_id, .. }
                if *pane_id == search.pane =>
            {
                if search.generation != 0 {
                    search.expire();
                    if let Some(screen) = self.model.panes.get_mut(pane_id) {
                        screen.search_highlight.clear();
                    }
                }
            }
            Message::TextQueryResult {
                id,
                pane_id,
                result,
            } if *pane_id == search.pane => {
                if matches!(result, TextResult::Search { .. }) {
                    if search.receive(*id, result)
                        && search.selected.is_none()
                        && !search.hits.is_empty()
                    {
                        self.navigate_search(false);
                    }
                } else if matches!(result, TextResult::Viewport { .. }) {
                    let Ok(picture) = api::text_viewport_snapshot(result) else {
                        return true;
                    };
                    if search.located(*id, result) {
                        if let TextResult::Viewport {
                            range, viewport, ..
                        } = result
                        {
                            if let Some(screen) = self.model.panes.get_mut(pane_id) {
                                screen.search_highlight =
                                    highlight(*range, viewport.offset, &picture);
                                screen.search_picture = Some(picture);
                                screen.search_returning = false;
                                screen.revision = screen.revision.saturating_add(1);
                            }
                        }
                    }
                }
                self.link_press.cancel();
                self.area.queue_render();
                return true;
            }
            _ => {}
        }
        false
    }
}

fn highlight(
    range: api::TextRange,
    offset: u32,
    picture: &api::Snapshot,
) -> Vec<(usize, usize, usize)> {
    let top = -(i64::from(offset));
    (0..usize::from(picture.rows))
        .filter_map(|y| {
            let row = top + y as i64;
            if row < i64::from(range.start.row) || row > i64::from(range.end.row) {
                return None;
            }
            let first = if row == i64::from(range.start.row) {
                usize::from(range.start.col)
            } else {
                0
            };
            let mut end = if row == i64::from(range.end.row) {
                usize::from(range.end.col)
            } else {
                usize::from(picture.cols)
            };
            if row == i64::from(range.end.row) && range.end.byte > 0 {
                end += picture
                    .screen
                    .get(y)
                    .and_then(|r| r.get(end))
                    .map_or(1, |c| usize::from(c.width.max(1)));
            }
            (first < end).then_some((y, first, end.min(usize::from(picture.cols))))
        })
        .collect()
}
