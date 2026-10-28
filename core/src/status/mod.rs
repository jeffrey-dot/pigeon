//! OSC 7501 records and their lifecycle.
//!
//! The one place that decides what a status report means and how long it lives.
//! Lifecycle rules come from the spec and from `AGENTS.md` §7:
//!   * `done` / `error` stay until the user has seen them (`ack`);
//!   * `working` / `blocked` die with the process or at an OSC 133A prompt;
//!   * at most [`MAX_RECORDS`] records per pane, evicting acked ones first.

mod parse;

use std::collections::BTreeMap;

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
}

/// Per-pane status store.
#[derive(Default)]
pub struct StatusStore {
    records: BTreeMap<String, Record>,
    buf: Vec<u8>,
    /// Tail of the stream used to spot OSC 133 `A` (a shell prompt) even when the
    /// marker is split across reads.
    prompt_buf: Vec<u8>,
    /// Last cwd reported by OSC 7 (falls back to the spawn cwd in the host).
    cwd: Option<String>,
    /// Last title reported by OSC 0/2.
    title: Option<String>,
    /// Bumped on every mutation, so the host can broadcast only on change.
    revision: u32,
    next_seq: u32,
}

/// OSC 133 `A` — the shell is back at a prompt.
const PROMPT_MARKER: &[u8] = b"\x1b]133;A";

impl StatusStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consumes pane bytes. Bytes that are not OSC 7501 are ignored here — the
    /// VT engine is what turns them into a screen.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.scan_prompts(bytes);
        self.buf.extend_from_slice(bytes);
        let mut sequences = Vec::new();
        parse::take_osc(&mut self.buf, &mut sequences);
        for (code, body) in sequences {
            match code.as_slice() {
                b"7501" => self.apply(&body),
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

    /// Marks whichever record carries `seq` as seen.
    pub fn ack_seq(&mut self, seq: u32) -> bool {
        let mut acked = false;
        for record in self.records.values_mut() {
            if record.seq == seq {
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

    /// Marks a record as seen by the user. Only the current `seq` can be acked.
    pub fn ack(&mut self, id: &str, seq: u32) -> bool {
        match self.records.get_mut(id) {
            Some(record) if record.seq == seq => {
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

    /// OSC 133 `A` means the prompt is back: nothing is working any more.
    fn scan_prompts(&mut self, bytes: &[u8]) {
        self.prompt_buf.extend_from_slice(bytes);
        while let Some(index) = find_bytes(&self.prompt_buf, PROMPT_MARKER) {
            self.prompt_buf.drain(..index + PROMPT_MARKER.len());
            self.on_prompt();
        }
        let keep = PROMPT_MARKER.len().saturating_sub(1);
        if self.prompt_buf.len() > keep {
            let drop = self.prompt_buf.len() - keep;
            self.prompt_buf.drain(..drop);
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
                let record = Record {
                    id: report.id.clone(),
                    state,
                    kind: Kind::parse(&report.kind),
                    msg: report.msg,
                    app: report.app,
                    seq: self.next_seq,
                    acked: false,
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

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::parse::MAX_SEQUENCE;
    use super::{Kind, State, StatusStore, MAX_RECORDS};

    fn report(fields: &str) -> Vec<u8> {
        format!("\x1b]7501;{fields}\x07").into_bytes()
    }

    #[test]
    fn report_creates_a_record() {
        let mut store = StatusStore::new();
        store.feed(&report("id=w1:state=blocked:kind=permission:app=pi"));
        let record = store.get("w1").expect("record");
        assert_eq!(record.state, State::Blocked);
        assert_eq!(record.kind, Kind::Permission);
        assert_eq!(record.app, "pi");
        assert_eq!(record.seq, 1);
        assert!(!record.acked);
    }

    #[test]
    fn update_bumps_seq_and_replaces_the_record() {
        let mut store = StatusStore::new();
        store.feed(&report("id=w1:state=working"));
        store.feed(&report("id=w1:state=done"));
        let record = store.get("w1").expect("record");
        assert_eq!(record.state, State::Done);
        assert_eq!(record.seq, 2);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn process_exit_clears_working_but_keeps_done() {
        let mut store = StatusStore::new();
        store.feed(&report("id=a:state=working"));
        store.feed(&report("id=b:state=blocked:kind=question"));
        store.feed(&report("id=c:state=done"));
        store.feed(&report("id=d:state=error"));

        store.on_process_exit();

        let left: Vec<&str> = store.records().map(|r| r.id.as_str()).collect();
        assert_eq!(left, vec!["c", "d"], "only done/error survive the process");
    }

    #[test]
    fn prompt_clears_transient_states() {
        let mut store = StatusStore::new();
        store.feed(&report("id=a:state=working"));
        store.feed(&report("id=b:state=done"));
        store.on_prompt();
        assert!(store.get("a").is_none());
        assert!(store.get("b").is_some(), "done waits for the user");
    }

    #[test]
    fn osc7_sets_the_cwd_and_bad_urls_are_ignored() {
        let mut store = StatusStore::new();
        store.feed(b"\x1b]7;file:///Users/me/pigeon\x07");
        assert_eq!(store.cwd(), Some("/Users/me/pigeon"));
        store.feed(b"\x1b]7;file://localhost/Users/me/pigeon%20core\x1b\\");
        assert_eq!(store.cwd(), Some("/Users/me/pigeon core"));
        // Non-file scheme, bad escape and a control character all keep the old value.
        store.feed(b"\x1b]7;http://example.com/x\x07");
        store.feed(b"\x1b]7;file:///bad%zzpath\x07");
        store.feed(b"\x1b]7;file:///bad\x01path\x07");
        assert_eq!(store.cwd(), Some("/Users/me/pigeon core"));
    }

    #[test]
    fn osc0_and_osc2_set_the_title() {
        let mut store = StatusStore::new();
        store.feed(b"\x1b]0;zsh\x07");
        assert_eq!(store.title(), Some("zsh"));
        store.feed(b"\x1b]2;vim /tmp/demo.txt\x07");
        assert_eq!(store.title(), Some("vim /tmp/demo.txt"));
        store.feed(b"\x1b]2;bad\x1btitle\x07");
        assert_eq!(
            store.title(),
            Some("vim /tmp/demo.txt"),
            "control chars dropped"
        );
    }

    #[test]
    fn osc133_prompt_clears_transient_states() {
        let mut store = StatusStore::new();
        store.feed(&report("id=a:state=working"));
        store.feed(&report("id=b:state=done"));
        // Split the prompt marker across two reads on purpose.
        store.feed(b"\x1b]13");
        store.feed(b"3;A");
        assert!(store.get("a").is_none(), "working dies at the prompt");
        assert!(store.get("b").is_some(), "done waits for the user");
    }

    #[test]
    fn ack_only_matches_the_current_seq() {
        let mut store = StatusStore::new();
        store.feed(&report("id=a:state=done"));
        assert!(!store.ack("a", 99), "stale seq is rejected");
        assert!(!store.ack("missing", 1));
        assert!(store.ack("a", 1));
        assert!(store.get("a").expect("record").acked);
    }

    #[test]
    fn clear_removes_a_subtree() {
        let mut store = StatusStore::new();
        store.feed(&report("id=a:state=working"));
        store.feed(&report("id=a.1:state=working"));
        store.feed(&report("id=a.1.x:state=done"));
        store.feed(&report("id=ab:state=done"));

        store.feed(&report("state=clear:id=a"));

        let left: Vec<&str> = store.records().map(|r| r.id.as_str()).collect();
        assert_eq!(left, vec!["ab"], "only the exact subtree goes away");
    }

    #[test]
    fn clear_without_id_removes_everything() {
        let mut store = StatusStore::new();
        store.feed(&report("id=a:state=done"));
        store.feed(&report("state=clear"));
        assert!(store.is_empty());
    }

    #[test]
    fn records_are_capped_and_acked_ones_go_first() {
        let mut store = StatusStore::new();
        for index in 0..MAX_RECORDS {
            store.feed(&report(&format!("id=r{index}:state=done")));
        }
        assert_eq!(store.len(), MAX_RECORDS);

        // Ack the oldest, then add one more: the acked one is the victim.
        assert!(store.ack("r0", 1));
        store.feed(&report("id=overflow:state=done"));

        assert_eq!(store.len(), MAX_RECORDS);
        assert!(store.get("r0").is_none(), "acked record was evicted first");
        assert!(store.get("r1").is_some());
        assert!(store.get("overflow").is_some());
    }

    #[test]
    fn overlong_sequence_never_becomes_a_record() {
        let mut store = StatusStore::new();
        let mut bytes = b"\x1b]7501;state=working".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', MAX_SEQUENCE + 100));
        store.feed(&bytes);
        assert!(store.is_empty());

        store.feed(&report("state=done"));
        assert_eq!(store.len(), 1, "the scanner recovers after the garbage");
    }
}
