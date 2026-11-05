use super::*;
#[test]
fn uri_policy_accepts_web_links_and_rejects_ambiguous_or_executable_targets() {
    for uri in [
        "https://example.com/a%20b?x=1#y",
        "http://[::1]:8080/",
        "HTTPS://example.com/中文",
    ] {
        assert!(opening_uri(uri).is_some(), "{uri}");
    }
    for uri in [
        "",
        "file:///tmp/x",
        "javascript:alert(1)",
        "https:///path",
        "https://a:0/",
        "https://a:65536/",
        "https://a:abc/",
        "https://a:/",
        "https://user:pass@host/",
        "https://@host/",
        "https://a/\\x",
        "https://a/%5c",
        "https://a/%00",
        "https://a/%0a",
        "https://a/%ff",
        "https://a/%",
        "https://a/a b",
        "https://a/\u{202e}",
        "https://a/%E2%80%AE",
        "https://a/%09",
        "https://a/\u{2067}",
    ] {
        assert!(opening_uri(uri).is_none(), "{uri}");
    }
}
#[test]
fn only_a_complete_same_target_click_opens_and_cancellation_is_sticky() {
    let target = LinkTarget {
        pane: 1,
        revision: 7,
        column: 2,
        row: 3,
        uri: "https://example.com/".into(),
    };
    let mut click = LinkPress::default();
    let mut opened = Vec::new();
    click.begin(Some(target.clone()));
    click.finish(Some(&target), true, |uri| opened.push(uri.to_owned()));
    assert_eq!(opened, vec![target.uri.clone()]);
    click.begin(Some(target.clone()));
    click.validate(None);
    click.validate(Some(&target));
    click.finish(Some(&target), true, |uri| opened.push(uri.to_owned()));
    click.begin(Some(target.clone()));
    click.cancel();
    click.finish(Some(&target), true, |uri| opened.push(uri.to_owned()));
    click.begin(Some(target.clone()));
    click.finish(Some(&target), false, |uri| opened.push(uri.to_owned()));
    click.begin(Some(target.clone()));
    let mut updated = target.clone();
    updated.revision += 1;
    click.validate(Some(&updated));
    click.finish(Some(&target), true, |uri| opened.push(uri.to_owned()));
    assert_eq!(opened.len(), 1);
    click.begin(Some(target));
    click.cancel();
    click.begin(None);
    assert!(!click.consumed);
}

#[test]
fn invisible_directional_formatting_is_rejected_raw_and_percent_encoded() {
    for uri in [
        "https://a/\u{061c}",
        "https://a/%D8%9C",
        "https://a/\u{2060}",
        "https://a/\u{00ad}",
    ] {
        assert!(opening_uri(uri).is_none(), "{uri}");
    }
}
