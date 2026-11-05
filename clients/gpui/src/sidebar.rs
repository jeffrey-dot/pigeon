//! Pure sidebar model: project grouping, ordering, labels, status and relative time.
//!
//! Grouping and age formatting are ported from the Linux client
//! (`clients/linux/src/sidebar/{grouping,rows}.rs`); the needs-you rule and group-name casing
//! follow the macOS `SidebarModel`. UI-derived logic with no toolkit dependency (AGENTS.md §3
//! keeps it in the client). The GPUI view renders these rows; it does not re-derive them.

use pigeon_core::api::SessionInfo;
use std::collections::BTreeMap;
use std::path::Path;

use crate::icons::StatusGlyph;
use crate::status::Summary;

/// Resolves the `(group key, display name)` for a session's cwd. The caller may pass a git
/// root; without one we group by the cwd itself and name it by its last path component.
pub fn project(cwd: &str, git_root: Option<&str>) -> (String, String) {
    let path = git_root.unwrap_or(cwd).trim_end_matches('/');
    let key = if path.is_empty() { "/" } else { path };
    let name = Path::new(key)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Local");
    (key.to_owned(), name.to_owned())
}

/// Harp capitalises only the first character of a group name (`iOS app` stays, `launchpad`
/// becomes `Launchpad`), as the macOS `displayGroupName` does.
pub fn display_group_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// One rendered sidebar line: either a project header or a session row.
#[derive(Debug, PartialEq, Eq)]
pub enum Row {
    Header {
        name: String,
        count: usize,
        /// Sessions that need the user: blocked, error, or unread done.
        needs: usize,
        /// The first group gets no gap above it.
        first: bool,
    },
    Session {
        session_id: u32,
        /// Plain-text label: active program/title, else a generated name (invariant 5).
        name: String,
        cwd: String,
        /// `None` when the pane never reported (no icon, no time: `docs/ui.md` §3.2).
        glyph: Option<StatusGlyph>,
        unread: bool,
        age: Option<String>,
    },
}

/// `N needs you` / `N need you`, as Harp and the macOS client word it.
pub fn needs_label(count: usize) -> String {
    if count == 1 {
        "1 needs you".to_owned()
    } else {
        format!("{count} need you")
    }
}

/// Groups sessions by project and returns the flat row list the sidebar draws: a header per
/// project (ordered by first session id), then its sessions ordered by session id.
pub fn rows(
    sessions: &[SessionInfo],
    git_roots: &BTreeMap<String, String>,
    summary: &dyn Fn(&[u32]) -> Summary,
) -> Vec<Row> {
    let mut groups: BTreeMap<String, (String, Vec<&SessionInfo>)> = BTreeMap::new();
    for session in sessions {
        let (key, name) = project(
            &session.cwd,
            git_roots.get(&session.cwd).map(String::as_str),
        );
        groups
            .entry(key)
            .or_insert_with(|| (name, Vec::new()))
            .1
            .push(session);
    }

    let mut ordered: Vec<_> = groups.into_values().collect();
    ordered.sort_by_key(|(_, sessions)| sessions.iter().map(|s| s.session_id).min());

    let mut rows = Vec::new();
    for (index, (name, mut sessions)) in ordered.into_iter().enumerate() {
        sessions.sort_by_key(|s| s.session_id);
        let summaries: Vec<Summary> = sessions.iter().map(|s| summary(&s.panes)).collect();
        rows.push(Row::Header {
            name: display_group_name(&name),
            count: sessions.len(),
            needs: summaries.iter().filter(|s| s.needs_you()).count(),
            first: index == 0,
        });
        for (session, summary) in sessions.into_iter().zip(summaries) {
            rows.push(Row::Session {
                session_id: session.session_id,
                name: session_label(session),
                cwd: session.cwd.clone(),
                glyph: StatusGlyph::for_state(summary.state, summary.unread > 0),
                unread: summary.unread > 0,
                age: summary.age_ms.map(age),
            });
        }
    }
    rows
}

/// The plain-text label for a session: the program name the host derived (OSC 0/2), else its
/// title, else a generated fallback.
pub fn session_label(session: &SessionInfo) -> String {
    session
        .active_program
        .clone()
        .or_else(|| session.active_title.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("session {}", session.session_id))
}

/// Relative age label (`now` / `1m` / `3h` / `2d`), matching `docs/ui.md` §3.2.
pub fn age(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pigeon_core::api::StatusState as State;

    fn session(id: u32, cwd: &str, program: Option<&str>) -> SessionInfo {
        SessionInfo {
            session_id: id,
            title: format!("title-{id}"),
            cwd: cwd.to_owned(),
            panes: vec![id],
            active_pane: None,
            active_title: None,
            active_program: program.map(str::to_owned),
        }
    }

    fn none(_: &[u32]) -> Summary {
        Summary::default()
    }

    #[test]
    fn git_subdirectories_share_a_group_but_same_names_do_not() {
        assert_eq!(
            project("/src/a/sub", Some("/src/a")),
            project("/src/a", Some("/src/a"))
        );
        assert_ne!(project("/src/a", None).0, project("/other/a", None).0);
        assert_eq!(project("/src/a/sub/", None).1, "sub");
    }

    #[test]
    fn rows_group_by_project_then_order_headers_and_sessions() {
        let sessions = vec![
            session(2, "/src/a", Some("vim")),
            session(1, "/src/b", None),
            session(3, "/src/a", Some("htop")),
        ];
        let rows = rows(&sessions, &BTreeMap::new(), &none);
        let plain = |id: u32, name: &str, cwd: &str| Row::Session {
            session_id: id,
            name: name.into(),
            cwd: cwd.into(),
            glyph: None,
            unread: false,
            age: None,
        };
        assert_eq!(
            rows,
            vec![
                Row::Header {
                    name: "B".into(),
                    count: 1,
                    needs: 0,
                    first: true
                },
                plain(1, "session 1", "/src/b"),
                Row::Header {
                    name: "A".into(),
                    count: 2,
                    needs: 0,
                    first: false
                },
                plain(2, "vim", "/src/a"),
                plain(3, "htop", "/src/a"),
            ]
        );
    }

    #[test]
    fn needs_counts_blocked_error_and_unread_done_only() {
        let sessions: Vec<_> = (1..=5).map(|id| session(id, "/p", None)).collect();
        let summary = |panes: &[u32]| match panes[0] {
            1 => Summary {
                state: Some(State::Blocked),
                unread: 0,
                age_ms: Some(0),
            },
            2 => Summary {
                state: Some(State::Done),
                unread: 1,
                age_ms: Some(240_000),
            },
            3 => Summary {
                state: Some(State::Done),
                unread: 0,
                age_ms: Some(0),
            },
            4 => Summary {
                state: Some(State::Working),
                unread: 0,
                age_ms: Some(60_000),
            },
            _ => Summary {
                state: Some(State::Error),
                unread: 0,
                age_ms: Some(0),
            },
        };
        let rows = rows(&sessions, &BTreeMap::new(), &summary);
        assert!(matches!(
            rows[0],
            Row::Header {
                needs: 3,
                count: 5,
                ..
            }
        ));
        assert!(matches!(
            &rows[2],
            Row::Session { glyph: Some(StatusGlyph::DoneUnread), unread: true, age: Some(a), .. }
                if a == "4m"
        ));
        assert!(matches!(
            &rows[3],
            Row::Session {
                glyph: Some(StatusGlyph::DoneRead),
                ..
            }
        ));
    }

    #[test]
    fn group_names_capitalise_only_the_first_letter() {
        assert_eq!(display_group_name("launchpad"), "Launchpad");
        assert_eq!(display_group_name("iOS app"), "IOS app");
        assert_eq!(display_group_name(""), "");
    }

    #[test]
    fn needs_label_wording() {
        assert_eq!(needs_label(1), "1 needs you");
        assert_eq!(needs_label(2), "2 need you");
    }

    #[test]
    fn session_label_prefers_program_then_title_then_fallback() {
        let mut s = session(7, "/x", Some("cargo"));
        assert_eq!(session_label(&s), "cargo");
        s.active_program = None;
        s.active_title = Some("neovim".into());
        assert_eq!(session_label(&s), "neovim");
        s.active_title = None;
        assert_eq!(session_label(&s), "session 7");
    }

    #[test]
    fn age_buckets_match_the_spec() {
        assert_eq!(age(0), "now");
        assert_eq!(age(90_000), "1m");
        assert_eq!(age(3_600_000 * 3), "3h");
        assert_eq!(age(86_400_000 * 2), "2d");
    }
}
