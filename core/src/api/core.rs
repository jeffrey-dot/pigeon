//! [`Core`]: the embedded session container behind both client faces.
//!
//! `ffi::PgCore` wraps exactly one of these, so the C ABI and Rust clients run
//! the same code path; C-only concerns (pointers, sizes, error codes) stay in
//! `ffi`.

use std::collections::BTreeMap;

use crate::error::{PgError, PgResult};
use crate::layout::{Axis, Layout, PaneId, Rect, SplitId};
use crate::session::PaneState;
use crate::status::Record;
use crate::term::Snapshot;

/// Core-wide settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Config {
    /// 0 = core default (10000 lines); capped at 100000.
    pub scrollback_lines: u32,
}

/// One embedded session: N panes plus the layout tree that places them.
pub struct Core {
    config: Config,
    panes: BTreeMap<PaneId, PaneState>,
    /// Geometry and focus live in the layout tree; the map only holds panes.
    layout: Option<Layout>,
    next_pane: PaneId,
}

impl Core {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            panes: BTreeMap::new(),
            layout: None,
            next_pane: 1,
        }
    }

    /// Creates the first pane. Further panes come from [`Core::split`], which
    /// knows the geometry, so a second call fails with `InvalidArg`.
    pub fn new_pane(&mut self, cols: u16, rows: u16) -> PgResult<PaneId> {
        // The id is consumed even on failure; C callers have always seen that.
        let pane_id = self.allocate_pane_id();
        if self.layout.is_some() {
            return Err(PgError::InvalidArg);
        }
        let pane = PaneState::new(pane_id, cols, rows, self.scrollback_lines());
        self.panes.insert(pane_id, pane);
        self.layout = Some(Layout::new(pane_id));
        Ok(pane_id)
    }

    /// Drops a pane. Unlike [`Core::close`], a layout that cannot close it
    /// (the last pane, say) is not an error.
    pub fn free_pane(&mut self, pane_id: PaneId) -> PgResult<()> {
        self.panes.remove(&pane_id).ok_or(PgError::NoSuchPane)?;
        if let Some(layout) = self.layout.as_mut() {
            let _ = layout.close(pane_id);
        }
        Ok(())
    }

    /// Parses bytes; never blocks, never echoes.
    pub fn feed(&mut self, pane_id: PaneId, bytes: &[u8]) -> PgResult<()> {
        self.pane_mut(pane_id)?.feed(bytes);
        Ok(())
    }

    pub fn resize(&mut self, pane_id: PaneId, cols: u16, rows: u16) -> PgResult<()> {
        self.pane_mut(pane_id)?.resize(cols, rows);
        Ok(())
    }

    /// Pending terminal replies (DA/DSR/…) the caller must write to the PTY.
    pub fn take_replies(&mut self, pane_id: PaneId) -> PgResult<Vec<u8>> {
        Ok(self.pane_mut(pane_id)?.take_replies())
    }

    /// Full picture of a pane, history included.
    pub fn snapshot(&mut self, pane_id: PaneId) -> PgResult<Snapshot> {
        Ok(self.pane_mut(pane_id)?.snapshot())
    }

    /// Row indices dirty since the last call.
    pub fn take_dirty_rows(&mut self, pane_id: PaneId) -> PgResult<Vec<u16>> {
        Ok(self.pane_mut(pane_id)?.take_damage())
    }

    /// The focused pane and its OSC 7501 records.
    pub fn focused_status(&self) -> PgResult<(PaneId, impl Iterator<Item = &Record>)> {
        let pane = self.focused_pane()?;
        Ok((pane.pane_id, pane.status().records()))
    }

    /// Marks the pane's default done/error record as seen; false when `seq`
    /// is not its current one.
    pub fn ack_status(&mut self, pane_id: PaneId, seq: u32) -> PgResult<bool> {
        Ok(self.pane_mut(pane_id)?.status_mut().ack("", seq))
    }

    /// Splits a pane and returns the **new pane id**, which takes focus.
    pub fn split(&mut self, pane_id: PaneId, axis: Axis, ratio: f32) -> PgResult<PaneId> {
        // The target pane's own size is the extent being divided.
        let (cols, rows) = self.pane(pane_id)?.size();
        let new_id = self.allocate_pane_id();
        let history = self.scrollback_lines();
        let layout = self.layout.as_mut().ok_or(PgError::NoSuchPane)?;
        layout.split(pane_id, new_id, axis, ratio, cols, rows)?;
        let (child_cols, child_rows) = match axis {
            Axis::Vertical => ((cols / 2).max(2), rows),
            Axis::Horizontal => (cols, (rows / 2).max(1)),
        };
        let mut pane = PaneState::new(new_id, child_cols, child_rows, history);
        pane.resize(child_cols, child_rows);
        self.panes.insert(new_id, pane);
        let _ = layout.focus(new_id);
        Ok(new_id)
    }

    /// Closes a pane; its sibling takes over the parent rect.
    pub fn close(&mut self, pane_id: PaneId) -> PgResult<()> {
        self.panes.remove(&pane_id).ok_or(PgError::NoSuchPane)?;
        if let Some(layout) = self.layout.as_mut() {
            layout.close(pane_id)?;
        }
        Ok(())
    }

    /// Changes a split's ratio, clamped so both sides keep their minimum size.
    pub fn set_split_ratio(&mut self, split_id: SplitId, ratio: f32) -> PgResult<()> {
        let (cols, rows) = self.focused_pane()?.size();
        let layout = self.layout.as_mut().ok_or(PgError::NoSuchSplit)?;
        layout.set_ratio(split_id, ratio, cols, rows)
    }

    /// One normalized rect per pane, in tree order.
    pub fn rects(&self) -> PgResult<Vec<(PaneId, Rect)>> {
        Ok(self.layout.as_ref().ok_or(PgError::NoSuchPane)?.rects())
    }

    /// The split to resize when the user drags the divider next to `pane_id`;
    /// `None` when the pane has no parent split.
    pub fn split_of(&self, pane_id: PaneId) -> PgResult<Option<SplitId>> {
        self.pane(pane_id)?;
        Ok(self
            .layout
            .as_ref()
            .and_then(|layout| layout.split_of(pane_id)))
    }

    pub fn focus(&mut self, pane_id: PaneId) -> PgResult<()> {
        let layout = self.layout.as_mut().ok_or(PgError::NoSuchPane)?;
        if layout.focus(pane_id) {
            Ok(())
        } else {
            Err(PgError::NoSuchPane)
        }
    }

    pub fn focused(&self) -> Option<PaneId> {
        self.layout.as_ref().and_then(Layout::focused)
    }

    fn allocate_pane_id(&mut self) -> PaneId {
        let pane_id = self.next_pane;
        self.next_pane = self.next_pane.saturating_add(1);
        pane_id
    }

    fn pane(&self, pane_id: PaneId) -> PgResult<&PaneState> {
        self.panes.get(&pane_id).ok_or(PgError::NoSuchPane)
    }

    fn pane_mut(&mut self, pane_id: PaneId) -> PgResult<&mut PaneState> {
        self.panes.get_mut(&pane_id).ok_or(PgError::NoSuchPane)
    }

    fn focused_pane(&self) -> PgResult<&PaneState> {
        self.pane(self.focused().ok_or(PgError::NoSuchPane)?)
    }

    fn scrollback_lines(&self) -> usize {
        match self.config.scrollback_lines {
            0 => 10_000,
            lines => usize::try_from(lines.min(100_000)).unwrap_or(10_000),
        }
    }
}
