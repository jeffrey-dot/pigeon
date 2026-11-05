//! Request identity and pagination for protocol text_query/text_viewport; no GTK or I/O.
use pigeon_core::api::{TextPoint, TextQuery, TextRange, TextResult};

#[cfg(test)]
mod tests;
pub mod view;

pub struct Search {
    pub pane: u32,
    pub selection: u64,
    pub pattern: String,
    pub sensitive: bool,
    pub generation: u64,
    pub hits: Vec<TextRange>,
    pub selected: Option<usize>,
    pub scanning: bool,
    pub notice: String,
    next: Option<TextPoint>,
    page: Option<u32>,
    location: Option<u32>,
}

impl Search {
    pub fn new(pane: u32, selection: u64) -> Self {
        Self {
            pane,
            selection,
            pattern: String::new(),
            sensitive: true,
            generation: 0,
            hits: vec![],
            selected: None,
            scanning: false,
            notice: "Enter literal text".into(),
            next: None,
            page: None,
            location: None,
        }
    }

    pub fn begin(&mut self, pattern: String, sensitive: bool) {
        self.pattern = pattern;
        self.sensitive = sensitive;
        self.generation = 0;
        self.hits.clear();
        self.selected = None;
        self.next = None;
        self.page = None;
        self.location = None;
        self.scanning = !self.pattern.is_empty()
            && self.pattern.len() <= 256
            && !self.pattern.chars().any(|c| c.is_control() && c != '\n');
        self.notice = if self.scanning {
            "Scanning…"
        } else if self.pattern.is_empty() {
            "Enter literal text"
        } else {
            "Use at most 256 UTF-8 bytes, without control characters"
        }
        .into();
    }

    pub fn page(&mut self, id: u32) -> Option<TextQuery> {
        if !self.scanning || self.page.is_some() {
            return None;
        }
        self.page = Some(id);
        Some(TextQuery::Search {
            generation: self.generation,
            pattern: self.pattern.clone(),
            case_sensitive: self.sensitive,
            max_results: 128,
            start: self.next,
        })
    }

    pub fn receive(&mut self, id: u32, result: &TextResult) -> bool {
        let TextResult::Search { info, hits, next } = result else {
            return false;
        };
        if self.page != Some(id) || !self.scanning {
            return false;
        }
        self.page = None;
        if self.generation != 0 && info.generation != self.generation {
            self.expire();
            return false;
        }
        self.generation = info.generation;
        let room = 10_000usize.saturating_sub(self.hits.len());
        self.hits.extend(hits.iter().take(room).copied());
        self.next = *next;
        let limited = hits.len() > room || (self.hits.len() == 10_000 && next.is_some());
        self.scanning = next.is_some() && !limited;
        self.notice = if limited {
            "10,000+ matches — refine search"
        } else if self.scanning {
            "Scanning…"
        } else {
            "Complete"
        }
        .into();
        true
    }

    pub fn navigate(&mut self, previous: bool, id: u32) -> Option<TextQuery> {
        if self.hits.is_empty() || self.generation == 0 {
            return None;
        }
        let len = self.hits.len();
        let selected = self
            .selected
            .map_or(if previous { len - 1 } else { 0 }, |old| {
                if previous {
                    (old + len - 1) % len
                } else {
                    (old + 1) % len
                }
            });
        self.selected = Some(selected);
        self.location = Some(id);
        Some(TextQuery::Viewport {
            generation: self.generation,
            range: self.hits[selected],
        })
    }

    pub fn located(&mut self, id: u32, result: &TextResult) -> bool {
        let TextResult::Viewport { info, range, .. } = result else {
            return false;
        };
        if self.location != Some(id)
            || info.generation != self.generation
            || self.selected.and_then(|i| self.hits.get(i)) != Some(range)
        {
            return false;
        }
        self.location = None;
        true
    }

    pub fn fail(&mut self, id: u32, code: &str) {
        if self.page != Some(id) && self.location != Some(id) {
            return;
        }
        if code == "stale_text" {
            self.expire();
        } else {
            self.cancel();
            self.notice = format!("Search unavailable ({code})");
        }
    }
    pub fn cancel(&mut self) {
        self.scanning = false;
        self.page = None;
        self.location = None;
        self.notice = "Stopped · Partial results".into();
    }
    pub fn expire(&mut self) {
        self.cancel();
        self.hits.clear();
        self.selected = None;
        self.next = None;
        self.generation = 0;
        self.notice = "Expired — Search Again".into();
    }
    pub fn cancellable(&self) -> bool {
        self.scanning || self.location.is_some()
    }
    pub fn needs_page(&self) -> bool {
        self.scanning && self.page.is_none()
    }
    pub fn summary(&self) -> String {
        format!(
            "{}{} matches · {}",
            self.selected
                .map_or(String::new(), |i| format!("{} / ", i + 1)),
            self.hits.len(),
            self.notice
        )
    }
}
