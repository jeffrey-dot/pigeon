//! Pure session grouping, ordering, labels and keyboard navigation.
use super::{priority, SidebarModel};
use pigeon_core::api::SessionInfo;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, PartialEq, Eq)]
pub struct SidebarRow {
    pub session: Option<u32>,
    pub text: String,
    pub tooltip: String,
    pub name: String,
    pub age: String,
    pub state: Option<pigeon_core::api::StatusState>,
    pub unread: bool,
    pub count: usize,
    pub needs: usize,
}

impl SidebarModel {
    pub fn rows(
        &self,
        sessions: &[SessionInfo],
        projects: &HashMap<String, (String, String)>,
        sizes: &HashMap<u32, (String, u8)>,
    ) -> Vec<SidebarRow> {
        let mut groups: BTreeMap<String, (String, Vec<_>)> = BTreeMap::new();
        for session in sessions {
            let (key, name) = projects
                .get(&session.cwd)
                .cloned()
                .unwrap_or_else(|| super::grouping::project(&session.cwd, None));
            groups
                .entry(key)
                .or_insert_with(|| (name, vec![]))
                .1
                .push(session);
        }
        let mut rows = Vec::new();
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by_key(|(_, (_, sessions))| sessions.iter().map(|s| s.session_id).min());
        for (path, (name, mut sessions)) in groups {
            sessions.sort_by_key(|s| {
                (
                    self.summary(&s.panes).state.map(priority).unwrap_or(5),
                    s.session_id,
                )
            });
            let needs = sessions
                .iter()
                .filter(|s| self.summary(&s.panes).needs_you)
                .count();
            rows.push(SidebarRow {
                session: None,
                text: format!(
                    "{name} · {} · {needs} {}",
                    sessions.len(),
                    if needs == 1 { "needs you" } else { "need you" }
                ),
                tooltip: path,
                name: name.clone(),
                age: String::new(),
                state: None,
                unread: false,
                count: sessions.len(),
                needs,
            });
            for session in sessions {
                let summary = self.summary(&session.panes);
                let state = summary.state.map(|s| s.as_str()).unwrap_or("");
                let age = summary.age_ms.map(age).unwrap_or_default();
                let unread = if summary.unread > 0 {
                    format!(" · {} unread", summary.unread)
                } else {
                    String::new()
                };
                let size = session
                    .panes
                    .iter()
                    .find_map(|p| sizes.get(p))
                    .map(|(source, clients)| format!(" · {clients}× {source}"))
                    .unwrap_or_default();
                let title = session
                    .active_title
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&session.title);
                rows.push(SidebarRow {
                    session: Some(session.session_id),
                    text: format!(
                        "{title} · #{}  {age} {state}{unread}{size}",
                        session.session_id
                    ),
                    tooltip: format!("{}\n{state}{unread}{size}", session.cwd),
                    name: title.to_owned(),
                    age,
                    state: summary.state,
                    unread: summary.unread > 0,
                    count: 0,
                    needs: 0,
                });
            }
        }
        rows
    }

    pub fn next_needed(&self, sessions: &[SessionInfo], selected: Option<u32>) -> Option<u32> {
        let current = sessions
            .iter()
            .position(|s| Some(s.session_id) == selected)
            .unwrap_or(0);
        (1..=sessions.len())
            .map(|step| &sessions[(current + step) % sessions.len()])
            .find(|s| self.summary(&s.panes).needs_you)
            .map(|s| s.session_id)
    }
}

pub fn adjacent(sessions: &[SessionInfo], selected: Option<u32>, previous: bool) -> Option<u32> {
    if sessions.is_empty() {
        return None;
    }
    let current = sessions
        .iter()
        .position(|s| Some(s.session_id) == selected)
        .unwrap_or(0);
    let next = if previous {
        (current + sessions.len() - 1) % sessions.len()
    } else {
        (current + 1) % sessions.len()
    };
    Some(sessions[next].session_id)
}

pub fn age(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}
