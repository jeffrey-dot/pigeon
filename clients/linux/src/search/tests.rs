use super::*;
use pigeon_core::api::{Scroll, TextInfo, TextViewport, WireCursor};

fn range() -> TextRange {
    TextRange {
        start: TextPoint {
            row: -5,
            col: 1,
            byte: 0,
        },
        end: TextPoint {
            row: -5,
            col: 4,
            byte: 0,
        },
    }
}
fn page(next: Option<TextPoint>) -> TextResult {
    TextResult::Search {
        info: TextInfo {
            generation: 4,
            first_row: -10,
            last_row: 1,
            cols: 8,
            alt: false,
        },
        hits: vec![range()],
        next,
    }
}
#[test]
fn cancelled_or_replaced_pages_never_update_results() {
    let mut search = Search::new(1, 2);
    search.begin("first".into(), true);
    assert!(search.page(1).is_some());
    search.begin("second".into(), false);
    assert!(search.page(2).is_some());
    assert!(!search.receive(1, &page(None)));
    search.cancel();
    assert!(!search.receive(2, &page(None)));
    assert!(search.hits.is_empty());
}
#[test]
fn empty_pages_continue_and_expiry_clears_old_coordinates() {
    let mut search = Search::new(1, 1);
    search.begin("x".into(), true);
    search.page(1);
    let mut result = page(Some(range().end));
    if let TextResult::Search { hits, .. } = &mut result {
        hits.clear();
    }
    assert!(search.receive(1, &result));
    assert!(search.scanning);
    assert!(matches!(
        search.page(2),
        Some(TextQuery::Search {
            generation: 4,
            start: Some(_),
            ..
        })
    ));
    search.fail(2, "stale_text");
    assert_eq!(search.generation, 0);
    assert!(search.navigate(false, 3).is_none());
}
#[test]
fn completed_pages_still_allow_cancelling_a_late_location() {
    let mut search = Search::new(1, 1);
    search.begin("x".into(), true);
    search.page(1);
    assert!(search.receive(1, &page(None)));
    assert!(!search.cancellable());
    assert!(search.navigate(false, 2).is_some());
    assert!(search.cancellable());
    let viewport = TextViewport {
        rows: 2,
        cursor: WireCursor {
            x: 0,
            y: 0,
            visible: false,
            shape: 0,
        },
        scroll: Scroll { top: 0, bottom: 1 },
        screen: vec![],
        text: vec![],
        links: None,
        modes: 0,
        offset: 5,
        history_len: 10,
    };
    let result = TextResult::Viewport {
        info: page(None).info(),
        range: range(),
        viewport: Box::new(viewport),
    };
    search.cancel();
    assert!(!search.located(2, &result));
}
#[test]
fn result_storage_has_a_visible_limit() {
    let mut search = Search::new(1, 1);
    search.begin("x".into(), true);
    for id in 1..=80 {
        search.page(id);
        let mut result = page(Some(range().end));
        if let TextResult::Search { hits, .. } = &mut result {
            *hits = vec![range(); 128];
        }
        search.receive(id, &result);
    }
    assert_eq!(search.hits.len(), 10_000);
    assert!(!search.scanning);
    assert!(search.notice.contains("10,000+"));
}
