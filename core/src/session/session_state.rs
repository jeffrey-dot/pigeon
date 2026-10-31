//! One session as the host keeps it: a layout tree, the panes its leaves point at, the
//! session box and the size vote that picks it (protocol §3.2 `layout`, §6).
//!
//! Generic over the pane handle `P` so this stays free of any runtime: the host stores a
//! channel to each pane's task, a test stores anything.

use std::collections::HashMap;

use super::pane_meta::PaneMeta;
use super::size_vote::SizeVote;
use crate::layout::{Axis, Layout, LayoutNode, Rect};
use crate::proto::{Message, Node, SessionInfo, SplitAxis, SplitNode};

pub struct SessionState<P> {
    pub layout: Layout,
    pub panes: HashMap<u32, P>,
    pub vote: SizeVote,
    /// Shell basename: the fallback pane label.
    shell: String,
    /// cwd the session was created with (OSC 7 overrides it per pane).
    cwd: String,
    /// The session box in cells, chosen by `vote`.
    cols: u16,
    rows: u16,
}

impl<P> SessionState<P> {
    /// A session with one pane. `shell` is the program path; only its basename is kept.
    pub fn new(pane_id: u32, pane: P, shell: &str, cwd: String, cols: u16, rows: u16) -> Self {
        let mut panes = HashMap::new();
        panes.insert(pane_id, pane);
        Self {
            layout: Layout::new(pane_id),
            panes,
            vote: SizeVote::default(),
            shell: shell_label(shell),
            cwd,
            cols,
            rows,
        }
    }

    /// The session box in cells.
    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// One pane's box in cells: its share of the session box, clamped to the minimums the
    /// VT needs (a 1×0 grid is not a terminal).
    pub fn pane_size(&self, rect: &Rect) -> (u16, u16) {
        let cols = ((rect.w * f32::from(self.cols)).round() as u16).max(2);
        let rows = ((rect.h * f32::from(self.rows)).round() as u16).max(1);
        (cols, rows)
    }

    /// Every pane with the grid it should have now.
    pub fn pane_sizes(&self) -> Vec<(u32, u16, u16)> {
        self.layout
            .rects()
            .into_iter()
            .map(|(pane_id, rect)| {
                let (cols, rows) = self.pane_size(&rect);
                (pane_id, cols, rows)
            })
            .collect()
    }

    /// The grid a new pane gets before the layout re-fits it: half the box along `axis`.
    pub fn split_size(&self, axis: Axis) -> (u16, u16) {
        match axis {
            Axis::Vertical => ((self.cols / 2).max(2), self.rows),
            Axis::Horizontal => (self.cols, (self.rows / 2).max(1)),
        }
    }

    /// The session box a client's `cols`×`rows` grid for `pane_id` implies, or `None` when
    /// the pane is not on screen in this layout.
    pub fn box_for(&self, pane_id: u32, cols: u16, rows: u16) -> Option<(u16, u16)> {
        let (_, rect) = self
            .layout
            .rects()
            .into_iter()
            .find(|(id, _)| *id == pane_id)?;
        let cols = if rect.w > 0.0 {
            ((f32::from(cols) / rect.w).round() as u16).max(2)
        } else {
            self.cols
        };
        let rows = if rect.h > 0.0 {
            ((f32::from(rows) / rect.h).round() as u16).max(1)
        } else {
            self.rows
        };
        Some((cols, rows))
    }

    /// Takes whatever box the vote asks for now. Returns whether the box changed, i.e.
    /// whether the panes must be re-fitted.
    pub fn refit(&mut self) -> bool {
        let Some(wanted) = self.vote.negotiated(self.size()) else {
            return false;
        };
        if wanted == self.size() {
            return false;
        }
        (self.cols, self.rows) = wanted;
        true
    }

    /// The `layout` message for this session.
    pub fn layout_message(&self, session_id: u32) -> Message {
        Message::Layout {
            session_id,
            tree: self
                .layout
                .describe()
                .map(to_proto_tree)
                .unwrap_or(Node::Leaf { leaf: 0 }),
            zoom: self.layout.zoomed(),
            focus: self.layout.focused(),
        }
    }

    /// One `size_state` per pane: its grid, who is in charge and how many clients watch.
    pub fn size_states(&self) -> Vec<Message> {
        let clients = self.vote.clients().min(usize::from(u8::MAX)) as u8;
        let source = self.vote.policy().as_str();
        self.pane_sizes()
            .into_iter()
            .map(|(pane_id, cols, rows)| Message::SizeState {
                pane_id,
                cols,
                rows,
                source: source.to_owned(),
                clients,
            })
            .collect()
    }

    /// This session's sidebar row, named after the focused pane.
    pub fn info(&self, session_id: u32, meta: &HashMap<u32, PaneMeta>) -> SessionInfo {
        let active = self.layout.focused();
        let info = active.and_then(|pane| meta.get(&pane));
        let program = info
            .and_then(PaneMeta::program)
            .map(str::to_owned)
            .unwrap_or_else(|| self.shell.clone());
        SessionInfo {
            session_id,
            title: format!("session {session_id}"),
            cwd: info
                .and_then(|info| info.cwd.clone())
                .unwrap_or_else(|| self.cwd.clone()),
            panes: self.layout.pane_ids(),
            active_pane: active,
            active_title: Some(
                info.and_then(|info| info.title.clone())
                    .unwrap_or_else(|| self.shell.clone()),
            ),
            active_program: Some(program),
        }
    }
}

/// The `layout` a session sends once its last pane is gone. Convention: `{leaf: 0}` means
/// "no panes left" (pane ids start at 1).
pub fn empty_layout_message(session_id: u32) -> Message {
    Message::Layout {
        session_id,
        tree: Node::Leaf { leaf: 0 },
        zoom: None,
        focus: None,
    }
}

fn shell_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

fn to_proto_tree(node: LayoutNode) -> Node {
    match node {
        LayoutNode::Leaf(pane) => Node::Leaf { leaf: pane },
        LayoutNode::Split {
            id,
            axis,
            ratio,
            a,
            b,
        } => Node::Split {
            split: SplitNode {
                id,
                axis: match axis {
                    Axis::Vertical => SplitAxis::Vertical,
                    Axis::Horizontal => SplitAxis::Horizontal,
                },
                ratio,
                a: Box::new(to_proto_tree(*a)),
                b: Box::new(to_proto_tree(*b)),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SizePolicy;

    fn session() -> SessionState<()> {
        SessionState::new(1, (), "/bin/zsh", "/home/me".into(), 80, 24)
    }

    #[test]
    fn a_vertical_split_halves_the_box_and_maps_back() {
        let mut s = session();
        assert_eq!(s.split_size(Axis::Vertical), (40, 24));
        s.layout.split(1, 2, Axis::Vertical, 0.5, 80, 24).unwrap();
        s.panes.insert(2, ());
        assert_eq!(s.pane_sizes(), vec![(1, 40, 24), (2, 40, 24)]);
        // A client drawing pane 2 at 50×30 implies a 100×30 session box.
        assert_eq!(s.box_for(2, 50, 30), Some((100, 30)));
        assert_eq!(s.size_states().len(), 2);
    }

    #[test]
    fn box_for_an_unknown_pane_is_none() {
        let s = session();
        assert_eq!(s.box_for(99, 50, 30), None);
    }

    #[test]
    fn refit_follows_the_vote_and_ignores_an_empty_one() {
        let mut s = session();
        s.vote.join(7, SizePolicy::Smallest, s.size());
        s.vote.report(7, (100, 40));
        assert!(s.refit());
        assert_eq!(s.size(), (100, 40));
        assert!(!s.refit(), "nothing changed");
        s.vote.leave(7);
        assert!(!s.refit(), "nobody votes: keep the box");
        assert_eq!(s.size(), (100, 40));
    }

    #[test]
    fn size_state_reports_policy_and_client_count() {
        let mut s = session();
        s.vote.join(1, SizePolicy::Locked, s.size());
        s.vote.join(2, SizePolicy::Latest, s.size());
        let states = s.size_states();
        assert!(matches!(
            states.as_slice(),
            [Message::SizeState { pane_id: 1, cols: 80, rows: 24, source, clients: 2 }]
                if source == "locked"
        ));
    }

    #[test]
    fn info_names_a_program_but_not_a_prompt() {
        let s = session();
        let mut meta = HashMap::new();
        meta.insert(
            1,
            PaneMeta {
                cwd: Some("/tmp".into()),
                title: Some("vim".into()),
            },
        );
        let row = s.info(1, &meta);
        assert_eq!(row.cwd, "/tmp");
        assert_eq!(row.active_program.as_deref(), Some("vim"));

        meta.insert(
            1,
            PaneMeta {
                cwd: None,
                title: Some("~/dev".into()),
            },
        );
        let row = s.info(1, &meta);
        assert_eq!(row.cwd, "/home/me", "no OSC 7: the launch cwd");
        assert_eq!(row.active_title.as_deref(), Some("~/dev"));
        assert_eq!(row.active_program.as_deref(), Some("zsh"));
    }

    #[test]
    fn an_empty_session_describes_itself_as_leaf_zero() {
        let mut s = session();
        s.layout.close(1).unwrap();
        assert!(s.layout.is_empty());
        let Message::Layout { tree, focus, .. } = s.layout_message(4) else {
            panic!("layout");
        };
        assert!(matches!(tree, Node::Leaf { leaf: 0 }));
        assert_eq!(focus, None);
        assert!(matches!(
            empty_layout_message(4),
            Message::Layout { session_id: 4, tree: Node::Leaf { leaf: 0 }, .. }
        ));
    }
}
