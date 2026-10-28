//! Byte-level half of the OSC handling: find `ESC ] <code> ; <body> ST|BEL`
//! sequences in a pane stream. Codes we act on: 7501 (status), 7 (cwd), 0/2
//! (title). Everything else is ignored.
//!
//! Spec rules implemented here (see `docs/plan.md` §4 and `AGENTS.md` §7):
//!   * an over-long, undecodable, or control-character-carrying report is dropped
//!     **whole** — never half-applied;
//!   * unknown fields and unknown states are ignored, not fatal;
//!   * the scanner keeps a bounded tail so a sequence split across reads still
//!     resolves exactly once.

/// `ESC ]`
pub(crate) const PREFIX: &[u8] = b"\x1b]";

/// Anything longer is not a report we are willing to buffer.
pub(crate) const MAX_SEQUENCE: usize = 4096;

/// One report, already decoded and validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub id: String,
    pub state: String,
    pub kind: String,
    pub msg: String,
    pub app: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parsed {
    /// A status record to store.
    Record(Report),
    /// Remove this record (and its subtree).
    Clear(String),
    /// Drop the sequence entirely.
    Ignore,
}

/// Pulls every complete OSC sequence out of `buf` as `(code, body)`, leaving
/// the unconsumed (possibly partial) tail in place.
pub(crate) fn take_osc(buf: &mut Vec<u8>, out: &mut Vec<(Vec<u8>, Vec<u8>)>) {
    loop {
        let Some(start) = find(buf, PREFIX) else {
            keep_prefix_tail(buf);
            return;
        };
        buf.drain(..start);

        let Some((body_end, consumed)) = terminator(&buf[PREFIX.len()..]) else {
            if buf.len() > MAX_SEQUENCE {
                // Never let a runaway sequence grow the buffer; the whole
                // sequence is unusable anyway.
                buf.clear();
            }
            return;
        };

        let body = buf[PREFIX.len()..PREFIX.len() + body_end].to_vec();
        buf.drain(..PREFIX.len() + consumed);
        let split = body.iter().position(|byte| *byte == b';');
        let (code, payload) = match split {
            Some(index) => (body[..index].to_vec(), body[index + 1..].to_vec()),
            None => (body, Vec::new()),
        };
        out.push((code, payload));
    }
}

/// `file://host/path` → decoded local path. Non-`file` schemes, bad escapes and
/// control characters drop the whole sequence.
pub(crate) fn decode_file_url(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let rest = text.strip_prefix("file://")?;
    let path_start = rest.find('/')?;
    let decoded = percent_decode(&rest[path_start..])?;
    if decoded.is_empty() || has_control_chars(&decoded) {
        return None;
    }
    Some(decoded)
}

/// Plain-text payload (title): UTF-8 without control characters.
pub(crate) fn plain_text(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    if text.is_empty() || has_control_chars(text) {
        return None;
    }
    Some(text.to_owned())
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let value = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub(crate) fn parse(body: &[u8]) -> Parsed {
    let mut state = None;
    let mut id = String::new();
    let mut kind = String::new();
    let mut msg = String::new();
    let mut app = String::new();

    for pair in body.split(|byte| *byte == b':') {
        let Some(eq) = pair.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let (key, value) = (&pair[..eq], &pair[eq + 1..]);
        if key.is_empty() || value.is_empty() {
            continue;
        }
        let (Ok(key), Ok(value)) = (std::str::from_utf8(key), std::str::from_utf8(value)) else {
            continue;
        };
        match key {
            "state" => state = Some(value.to_owned()),
            "id" => id = value.to_owned(),
            "kind" => kind = value.to_owned(),
            "app" => {
                if !has_control_chars(value) {
                    app = value.to_owned();
                }
            }
            "msg" => match decode_message(value) {
                Some(text) => msg = text,
                // A message that does not decode takes the whole report down.
                None => return Parsed::Ignore,
            },
            // Unknown fields are forward compatible by design.
            _ => {}
        }
    }

    match state.as_deref() {
        Some("clear") => Parsed::Clear(id),
        Some("idle" | "working" | "blocked" | "done" | "error") => Parsed::Record(Report {
            id,
            state: state.unwrap_or_default(),
            kind,
            msg,
            app,
        }),
        // Support probes (`state=?`) and anything unknown are ignored.
        _ => Parsed::Ignore,
    }
}

fn decode_message(encoded: &str) -> Option<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    let mut padded = encoded.to_owned();
    let remainder = padded.len() % 4;
    if remainder != 0 {
        padded.push_str(&"=".repeat(4 - remainder));
    }
    let bytes = STANDARD.decode(padded.as_bytes()).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    (!has_control_chars(&text)).then_some(text)
}

/// C0, DEL and C1 are never allowed in text that reaches the UI.
fn has_control_chars(text: &str) -> bool {
    text.chars()
        .any(|c| c < ' ' || ('\u{7f}'..='\u{9f}').contains(&c))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Body length and total length (terminator included) for the first `BEL` or
/// `ESC \` at or after the start of the body.
fn terminator(buf: &[u8]) -> Option<(usize, usize)> {
    let mut index = 0;
    while index < buf.len() {
        if buf[index] == 0x07 {
            return Some((index, index + 1));
        }
        if buf[index] == 0x1b && buf.get(index + 1) == Some(&b'\\') {
            return Some((index, index + 2));
        }
        index += 1;
    }
    None
}

/// Keeps only the longest suffix that could still be the start of a prefix.
fn keep_prefix_tail(buf: &mut Vec<u8>) {
    let longest = PREFIX.len().saturating_sub(1).min(buf.len());
    for len in (1..=longest).rev() {
        if buf.ends_with(&PREFIX[..len]) {
            let keep_from = buf.len() - len;
            buf.drain(..keep_from);
            return;
        }
    }
    buf.clear();
}

#[cfg(test)]
mod tests {
    use super::{parse, take_osc, Parsed};

    fn bodies(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut buf = Vec::new();
        let mut sequences = Vec::new();
        for chunk in chunks {
            buf.extend_from_slice(chunk);
            take_osc(&mut buf, &mut sequences);
        }
        sequences
            .into_iter()
            .filter(|(code, _)| code.as_slice() == b"7501")
            .map(|(_, body)| body)
            .collect()
    }

    fn base64(text: &str) -> String {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine as _;
        STANDARD.encode(text.as_bytes())
    }

    #[test]
    fn complete_sequence_yields_its_body() {
        let found = bodies(&[b"\x1b]7501;state=working\x07"]);
        assert_eq!(found, vec![b"state=working".to_vec()]);
        // ESC \ is the other legal terminator.
        let found = bodies(&[b"\x1b]7501;state=done\x1b\\"]);
        assert_eq!(found, vec![b"state=done".to_vec()]);
    }

    #[test]
    fn sequence_split_across_chunks_is_assembled_once() {
        let found = bodies(&[b"\x1b]75", b"01;state=work", b"ing\x07"]);
        assert_eq!(found, vec![b"state=working".to_vec()]);
    }

    #[test]
    fn overlong_unterminated_sequence_is_discarded() {
        let mut buf = Vec::new();
        let mut out = Vec::new();
        buf.extend_from_slice(b"\x1b]7501;state=working");
        buf.extend(std::iter::repeat_n(b'x', 5000));
        take_osc(&mut buf, &mut out);
        assert!(out.is_empty());
        assert!(buf.is_empty(), "the runaway sequence is not kept");

        // The scanner recovers: a later, well-formed report still lands.
        buf.extend_from_slice(b"\x1b]7501;state=done\x07");
        take_osc(&mut buf, &mut out);
        assert_eq!(out, vec![(b"7501".to_vec(), b"state=done".to_vec())]);
    }

    #[test]
    fn valid_report_decodes_all_fields() {
        let msg = base64("Allow bash?");
        let body = format!("state=blocked:kind=permission:id=w1:app=pi:msg={msg}");
        let Parsed::Record(report) = parse(body.as_bytes()) else {
            panic!("expected a record");
        };
        assert_eq!(report.state, "blocked");
        assert_eq!(report.kind, "permission");
        assert_eq!(report.id, "w1");
        assert_eq!(report.app, "pi");
        assert_eq!(report.msg, "Allow bash?");
    }

    #[test]
    fn undecodable_base64_drops_the_whole_report() {
        let parsed = parse(b"state=blocked:kind=permission:msg=***not-base64***");
        assert_eq!(parsed, Parsed::Ignore);
    }

    #[test]
    fn control_characters_in_the_message_drop_the_whole_report() {
        let encoded = base64("ok\u{7}bad");
        let parsed = parse(format!("state=working:msg={encoded}").as_bytes());
        assert_eq!(parsed, Parsed::Ignore);
    }

    #[test]
    fn unknown_state_is_ignored() {
        assert_eq!(parse(b"state=spinning"), Parsed::Ignore);
        assert_eq!(parse(b"state=?"), Parsed::Ignore);
        assert_eq!(parse(b"app=pi"), Parsed::Ignore);
    }

    #[test]
    fn clear_carries_the_id() {
        assert_eq!(parse(b"state=clear:id=w1"), Parsed::Clear("w1".to_owned()));
    }

    #[test]
    fn malformed_pairs_are_skipped_not_fatal() {
        let Parsed::Record(report) = parse(b"nonsense:state=idle:kind=:id=w2") else {
            panic!("expected a record");
        };
        assert_eq!(report.state, "idle");
        assert_eq!(report.id, "w2");
    }
}
