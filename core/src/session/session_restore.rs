//! Saving a session for `SavedState` and bringing it back after a host restart.

use std::collections::HashMap;

use super::pane_meta::PaneMeta;
use super::saved_state::SavedSession;
use super::session_state::SessionState;
use crate::error::{PgError, PgResult};
use crate::layout::{Axis, Layout, LayoutNode};
use crate::proto::{Message, Node, SplitAxis};

impl<P> SessionState<P> {
    /// This session as the state file keeps it. `screens` gives each pane's `snapshot`
    /// message; a pane without one is left out, and a session left without panes is `None`.
    pub fn saved(
        &self,
        session_id: u32,
        meta: &HashMap<u32, PaneMeta>,
        screens: &HashMap<u32, Message>,
    ) -> Option<SavedSession> {
        let ids = self.layout.pane_ids();
        if ids.is_empty() || ids.iter().any(|pane| !screens.contains_key(pane)) {
            return None;
        }
        let Message::Layout { tree, focus, .. } = self.layout_message(session_id) else {
            return None;
        };
        Some(SavedSession {
            session_id,
            cwd: self.cwd().to_owned(),
            shell: self.shell().to_owned(),
            tree,
            focus,
            panes: ids
                .into_iter()
                .filter_map(|pane_id| {
                    let info = meta.get(&pane_id);
                    Some(super::saved_state::SavedPane {
                        pane_id,
                        cwd: info.and_then(|info| info.cwd.clone()),
                        title: info.and_then(|info| info.title.clone()),
                        screen: screens.get(&pane_id)?.clone(),
                    })
                })
                .collect(),
        })
    }

    /// A session rebuilt from `saved`, its panes renumbered through `renumber` (old id →
    /// new id) and handled by `panes` (new id → handle).
    pub fn restored(
        saved: &SavedSession,
        renumber: &HashMap<u32, u32>,
        panes: HashMap<u32, P>,
        cols: u16,
        rows: u16,
    ) -> PgResult<Self> {
        let layout = Layout::from_described(&to_layout(&saved.tree), renumber, saved.focus)?;
        if layout.pane_ids().iter().any(|pane| !panes.contains_key(pane)) {
            return Err(PgError::InvalidArg);
        }
        Ok(Self::from_parts(layout, panes, &saved.shell, saved.cwd.clone(), cols, rows))
    }
}

fn to_layout(node: &Node) -> LayoutNode {
    match node {
        Node::Leaf { leaf } => LayoutNode::Leaf(*leaf),
        Node::Split { split } => LayoutNode::Split {
            id: split.id,
            axis: match split.axis {
                SplitAxis::Vertical => Axis::Vertical,
                SplitAxis::Horizontal => Axis::Horizontal,
            },
            ratio: split.ratio,
            a: Box::new(to_layout(&split.a)),
            b: Box::new(to_layout(&split.b)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{PaneState, SavedState};

    fn screen(pane_id: u32) -> Message {
        PaneState::new(pane_id, 20, 4, 0).snapshot_message(false)
    }

    #[test]
    fn a_split_session_saves_and_comes_back_under_new_ids() {
        let mut session: SessionState<()> = SessionState::new(5, (), "/bin/zsh", "/w".into(), 80, 24);
        session
            .layout
            .split(5, 6, Axis::Vertical, 0.25, 80, 24)
            .expect("split");
        session.panes.insert(6, ());
        session.layout.focus(6);
        let meta = HashMap::from([(
            6,
            PaneMeta {
                cwd: Some("/w/sub".into()),
                title: Some("vim".into()),
            },
        )]);
        let screens = HashMap::from([(5, screen(5)), (6, screen(6))]);
        let saved = session.saved(3, &meta, &screens).expect("saved");
        assert_eq!(saved.shell, "zsh");
        assert_eq!(saved.panes[1].title.as_deref(), Some("vim"));
        // Round-trips through the file format's own checks.
        let bytes = SavedState::new(vec![saved.clone()]).encode().expect("encode");
        assert!(SavedState::decode(&bytes).is_ok());

        let renumber = HashMap::from([(5, 41), (6, 42)]);
        let back: SessionState<()> =
            SessionState::restored(&saved, &renumber, HashMap::from([(41, ()), (42, ())]), 80, 24)
                .expect("restored");
        assert_eq!(back.layout.pane_ids(), vec![41, 42]);
        assert_eq!(back.layout.focused(), Some(42));
        assert_eq!(back.cwd(), "/w");
    }

    #[test]
    fn a_session_missing_a_screen_or_a_pane_is_not_saved_or_restored() {
        let session: SessionState<()> = SessionState::new(5, (), "zsh", "/w".into(), 80, 24);
        assert!(session.saved(1, &HashMap::new(), &HashMap::new()).is_none());
        let saved = session
            .saved(1, &HashMap::new(), &HashMap::from([(5, screen(5))]))
            .expect("saved");
        // The tree names pane 5, but no handle is provided for its new id.
        let restored: PgResult<SessionState<()>> =
            SessionState::restored(&saved, &HashMap::from([(5, 9)]), HashMap::new(), 80, 24);
        assert!(restored.is_err());
    }
}
