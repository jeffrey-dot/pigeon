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
    store.feed(b"3;A\x07");
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
fn acking_a_live_state_keeps_it_visible() {
    // A client acks whatever it is looking at; `blocked` must survive that until the
    // agent moves on, otherwise the session the user is watching never shows it.
    let mut store = StatusStore::new();
    store.feed(&report("id=a:state=blocked:kind=permission"));
    assert!(!store.ack_seq(1), "blocked is not something to acknowledge");
    assert!(!store.ack("a", 1));
    assert_eq!(store.latest_unacked().map(|r| r.state), Some(State::Blocked));
    store.feed(&report("id=a:state=working"));
    assert!(!store.ack_seq(2));
    assert_eq!(store.latest_unacked().map(|r| r.state), Some(State::Working));
    store.feed(&report("id=a:state=done"));
    assert!(store.ack_seq(3), "done is acknowledged once seen");
    assert!(store.latest_unacked().is_none());
}

#[test]
fn a_repeated_state_keeps_its_start_time() {
    let mut store = StatusStore::new();
    store.feed(&report("id=a:state=working:msg=b25l"));
    let started = store.get("a").expect("record").since;
    std::thread::sleep(std::time::Duration::from_millis(5));
    store.feed(&report("id=a:state=working:msg=dHdv"));
    assert_eq!(store.get("a").expect("record").since, started, "same state, same clock");
    store.feed(&report("id=a:state=blocked"));
    assert!(store.get("a").expect("record").since > started, "a new state restarts it");
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

#[test]
fn overlong_complete_sequence_never_becomes_a_record() {
    let mut store = StatusStore::new();
    let mut bytes = b"\x1b]7501;state=working:id=".to_vec();
    bytes.extend(std::iter::repeat_n(b'x', MAX_SEQUENCE));
    bytes.push(0x07);
    store.feed(&bytes);
    assert!(store.is_empty(), "a terminated over-limit report is dropped too");
}

#[test]
fn control_characters_in_app_drop_the_whole_report() {
    let mut store = StatusStore::new();
    store.feed(&report("id=a:state=blocked:app=p\u{1}i"));
    assert!(store.is_empty(), "no half-applied record without its app");
}

const PROMPT: &[u8] = b"\x1b]133;A\x07";

fn feed_all(store: &mut StatusStore, chunks: &[&[u8]]) {
    for chunk in chunks {
        store.feed(chunk);
    }
}

#[test]
fn prompt_after_working_in_one_read_clears_it() {
    let mut store = StatusStore::new();
    let bytes = [report("id=a:state=working"), PROMPT.to_vec()].concat();
    store.feed(&bytes);
    assert!(store.get("a").is_none(), "the prompt came after the report");
}

#[test]
fn working_after_prompt_in_one_read_survives() {
    let mut store = StatusStore::new();
    let bytes = [PROMPT.to_vec(), report("id=a:state=working")].concat();
    store.feed(&bytes);
    assert!(store.get("a").is_some(), "the report came after the prompt");
}

#[test]
fn prompt_after_working_split_across_reads_clears_it() {
    let mut store = StatusStore::new();
    let bytes = [report("id=a:state=working"), PROMPT.to_vec()].concat();
    for cut in 1..bytes.len() {
        let mut store_cut = StatusStore::new();
        feed_all(&mut store_cut, &[&bytes[..cut], &bytes[cut..]]);
        assert!(store_cut.get("a").is_none(), "cut at {cut}");
    }
    store.feed(&bytes);
    assert!(store.get("a").is_none());
}

#[test]
fn working_after_prompt_split_across_reads_survives() {
    let bytes = [PROMPT.to_vec(), report("id=a:state=working")].concat();
    for cut in 1..bytes.len() {
        let mut store = StatusStore::new();
        feed_all(&mut store, &[&bytes[..cut], &bytes[cut..]]);
        assert!(store.get("a").is_some(), "cut at {cut}");
    }
}

#[test]
fn prompt_with_options_and_st_terminator_counts() {
    let mut store = StatusStore::new();
    store.feed(&report("id=a:state=blocked"));
    store.feed(b"\x1b]133;A;cl=m\x1b\\");
    assert!(store.get("a").is_none());
}

#[test]
fn other_osc133_marks_are_not_prompts() {
    let mut store = StatusStore::new();
    store.feed(&report("id=a:state=working"));
    store.feed(b"\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07\x1b]133;AB\x07");
    assert!(store.get("a").is_some(), "only `A` is a prompt");
}
