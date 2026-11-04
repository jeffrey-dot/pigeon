//! OSC 7501 records and their lifecycle.
//!
//! The one place that decides what a status report means and how long it lives.
//! Lifecycle rules come from the spec and from `AGENTS.md` §7:
//!   * `done` / `error` stay until the user has seen them (`ack`); acking a live
//!     `working` / `blocked` record is a no-op;
//!   * `working` / `blocked` die with the process or at an OSC 133A prompt;
//!   * at most [`MAX_RECORDS`] records per pane, evicting acked ones first.

mod parse;

use std::collections::BTreeMap;
use std::time::Instant;

/// At most this many records per pane.
pub const MAX_RECORDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Idle,
    Working,
    Blocked,
    Done,
    Error,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "blocked" => Self::Blocked,
            "done" => Self::Done,
            "error" => Self::Error,
            _ => return None,
        })
    }

    /// Records that survive their process exiting.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    None,
    Permission,
    Question,
    Auth,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "permission" => Self::Permission,
            "question" => Self::Question,
            "auth" => Self::Auth,
            _ => Self::None,
        }
    }
}

/// One status record. `id` lets a program keep several records per pane; an
/// empty id is the pane's default record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub state: State,
    pub kind: Kind,
    /// Plain text; control characters never reach this field.
    pub msg: String,
    pub app: String,
    /// Monotonic per store, bumped on every update.
    pub seq: u32,
    /// True once the user has seen this record.
    pub acked: bool,
    /// When the record entered `state`. A repeated report of the same state (an agent
    /// refreshing its `msg`) keeps the original time, so "working · 3m" keeps counting.
    pub since: Instant,
}

/// Per-pane status store.
#[derive(Default)]
pub struct StatusStore {
    records: BTreeMap<String, Record>,
    buf: Vec<u8>,
    /// Last cwd reported by OSC 7 (falls back to the spawn cwd in the host).
    cwd: Option<String>,
    /// Last title reported by OSC 0/2.
    title: Option<String>,
    /// Bumped on every mutation, so the host can broadcast only on change.
    revision: u32,
    next_seq: u32,
}

impl StatusStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes pane bytes. Bytes that are not OSC 7501 are ignored here — the
    /// VT engine is what turns them into a screen.
    ///
    /// Every sequence, OSC 133 included, comes out of one scanner and is applied in
    /// stream order, so a prompt only clears reports that were written before it.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
        let mut sequences = Vec::new();
        parse::take_osc(&mut self.buf, &mut sequences);
        for (code, body) in sequences {
            match code.as_slice() {
                b"7501" => self.apply(&body),
                b"133" if is_prompt_mark(&body) => self.on_prompt(),
                b"7" => {
                    if let Some(path) = parse::decode_file_url(&body) {
                        self.cwd = Some(path);
                    }
                }
                b"0" | b"2" => {
                    if let Some(text) = parse::plain_text(&body) {
                        self.title = Some(text);
                    }
                }
                _ => {}
            }
        }
    }

    /// The most recently updated record (highest `seq`), if any.
    pub fn latest(&self) -> Option<&Record> {
        self.records.values().max_by_key(|record| record.seq)
    }

    /// Highest `seq` record that the user has not seen yet.
    pub fn latest_unacked(&self) -> Option<&Record> {
        self.records
            .values()
            .filter(|record| !record.acked)
            .max_by_key(|record| record.seq)
    }

    /// Changes whenever the record set or any record changes.
    pub fn revision(&self) -> u32 {
        self.revision
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Marks whichever record carries `seq` as seen. Only `done`/`error` can be seen: a
    /// `working`/`blocked` record is the agent's live state and lasts until the agent, its
    /// process or a prompt replaces it, however often a client that is looking acks it.
    pub fn ack_seq(&mut self, seq: u32) -> bool {
        let mut acked = false;
        for record in self.records.values_mut() {
            if record.seq == seq && record.state.is_terminal() {
                record.acked = true;
                acked = true;
            }
        }
        if acked {
            self.touch();
        }
        acked
    }

    /// cwd last announced by the program (OSC 7), if any.
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// Title announced by the program (OSC 0/2), if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.records.values()
    }

    pub fn get(&self, id: &str) -> Option<&Record> {
        self.records.get(id)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Marks a record as seen by the user. Only the current `seq` of a `done`/`error`
    /// record can be acked (see [`StatusStore::ack_seq`]).
    pub fn ack(&mut self, id: &str, seq: u32) -> bool {
        match self.records.get_mut(id) {
            Some(record) if record.seq == seq && record.state.is_terminal() => {
                record.acked = true;
                true
            }
            _ => false,
        }
    }

    /// The pane's process is gone: transient states die, finished ones stay
    /// until the user looks at them.
    pub fn on_process_exit(&mut self) {
        self.retain(|record| record.state.is_terminal());
    }

    /// OSC 133A prompt: the shell is back, so nothing is "working" any more.
    pub fn on_prompt(&mut self) {
        self.retain(|record| record.state.is_terminal());
    }

    fn retain(&mut self, keep: impl Fn(&Record) -> bool) {
        let before = self.records.len();
        self.records.retain(|_, record| keep(record));
        if self.records.len() != before {
            self.touch();
        }
    }

    fn apply(&mut self, body: &[u8]) {
        match parse::parse(body) {
            parse::Parsed::Ignore => {}
            parse::Parsed::Clear(id) => self.clear(&id),
            parse::Parsed::Record(report) => {
                let Some(state) = State::parse(&report.state) else {
                    return;
                };
                self.next_seq = self.next_seq.saturating_add(1);
                let since = match self.records.get(&report.id) {
                    Some(previous) if previous.state == state => previous.since,
                    _ => Instant::now(),
                };
                let record = Record {
                    id: report.id.clone(),
                    state,
                    kind: Kind::parse(&report.kind),
                    msg: report.msg,
                    app: report.app,
                    seq: self.next_seq,
                    acked: false,
                    since,
                };
                self.records.insert(report.id, record);
                self.touch();
                self.evict();
            }
        }
    }

    fn clear(&mut self, id: &str) {
        if id.is_empty() {
            if !self.records.is_empty() {
                self.records.clear();
                self.touch();
            }
            return;
        }
        // `a` clears `a` and everything under `a.`; no other prefix matches.
        let subtree = format!("{id}.");
        let doomed: Vec<String> = self
            .records
            .keys()
            .filter(|key| key.as_str() == id || key.starts_with(&subtree))
            .cloned()
            .collect();
        if !doomed.is_empty() {
            self.touch();
        }
        for key in doomed {
            self.records.remove(&key);
        }
    }

    /// Drops the oldest acked record first, then the oldest overall.
    fn evict(&mut self) {
        while self.records.len() > MAX_RECORDS {
            let victim = self
                .records
                .values()
                .filter(|record| record.acked)
                .min_by_key(|record| record.seq)
                .or_else(|| self.records.values().min_by_key(|record| record.seq))
                .map(|record| record.id.clone());
            match victim {
                Some(id) => {
                    self.records.remove(&id);
                    self.touch();
                }
                None => break,
            }
        }
    }
}

/// OSC 133 `A` (optionally followed by `;options`) marks a fresh shell prompt.
fn is_prompt_mark(body: &[u8]) -> bool {
    body == b"A" || body.starts_with(b"A;")
}

#[cfg(test)]
mod tests;
