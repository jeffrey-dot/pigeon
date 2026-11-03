//! One connection's outbound frames: control frames in order, live screens coalesced.
//!
//! Control frames (replies, `layout`, `size_state`, `status`, `pane_exited`, `sessions`)
//! keep their order and are never dropped. Live screen updates only matter as the newest
//! screen, so each pane has at most one pending entry. Two deltas cannot be merged into one,
//! because the client requires contiguous `seq` (protocol §3.3), so when a delta arrives
//! while the pane's previous one is still queued, both become a marker that the writer turns
//! into a fresh snapshot when it reaches it. The marker moves to where the newest update
//! arrived, so a snapshot after a resize still follows the `layout` that announced it.
//!
//! Memory is bounded by `cap` bytes of encoded frames; markers cost nothing until resolved,
//! and there is at most one per pane.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// An encoded frame (length prefix included), shared by every connection that sends it.
pub type Frame = Arc<[u8]>;

/// What the writer sends next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    Frame(Frame),
    /// The pane's screen as it is when the writer gets here.
    Snapshot(u32),
}

/// The queue holds more than `cap` bytes: the connection is too slow to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverCap {
    pub queued: usize,
    pub cap: usize,
}

enum Slot {
    /// A pane's entry that a newer update replaced.
    Gone,
    Control(Frame),
    Delta {
        pane_id: u32,
        frame: Frame,
    },
    Snapshot {
        pane_id: u32,
    },
}

pub struct OutQueue {
    slots: VecDeque<Slot>,
    /// Position of `slots[0]`. Positions only grow, so a pane's entry can be found again
    /// after pops in front of it.
    head: u64,
    /// Each pane's pending live entry, by position.
    live: HashMap<u32, u64>,
    gone: usize,
    bytes: usize,
    cap: usize,
    closed: bool,
}

impl OutQueue {
    pub fn new(cap: usize) -> Self {
        Self {
            slots: VecDeque::new(),
            head: 0,
            live: HashMap::new(),
            gone: 0,
            bytes: 0,
            cap,
            closed: false,
        }
    }

    pub fn push_control(&mut self, frame: Frame) -> Result<(), OverCap> {
        self.bytes += frame.len();
        self.push(Slot::Control(frame));
        self.check()
    }

    /// A live delta: queued as is, or folded into a snapshot marker when the pane already
    /// has an update waiting.
    pub fn push_delta(&mut self, pane_id: u32, frame: Frame) -> Result<(), OverCap> {
        if self.live.contains_key(&pane_id) {
            self.push_snapshot(pane_id);
            return Ok(());
        }
        self.bytes += frame.len();
        let position = self.push(Slot::Delta { pane_id, frame });
        self.live.insert(pane_id, position);
        self.check()
    }

    /// The pane needs a fresh snapshot; it replaces whatever update the pane had waiting.
    pub fn push_snapshot(&mut self, pane_id: u32) {
        self.retire(pane_id);
        let position = self.push(Slot::Snapshot { pane_id });
        self.live.insert(pane_id, position);
        self.compact_if_sparse();
    }

    pub fn pop(&mut self) -> Option<Outgoing> {
        while let Some(slot) = self.slots.pop_front() {
            self.head += 1;
            match slot {
                Slot::Gone => self.gone -= 1,
                Slot::Control(frame) => {
                    self.bytes -= frame.len();
                    return Some(Outgoing::Frame(frame));
                }
                Slot::Delta { pane_id, frame } => {
                    self.live.remove(&pane_id);
                    self.bytes -= frame.len();
                    return Some(Outgoing::Frame(frame));
                }
                Slot::Snapshot { pane_id } => {
                    self.live.remove(&pane_id);
                    return Some(Outgoing::Snapshot(pane_id));
                }
            }
        }
        None
    }

    /// Encoded bytes waiting (markers excluded).
    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Entries still to send.
    pub fn len(&self) -> usize {
        self.slots.len() - self.gone
    }

    /// No more frames will be pushed; the writer sends what is left and stops.
    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    fn push(&mut self, slot: Slot) -> u64 {
        let position = self.head + self.slots.len() as u64;
        self.slots.push_back(slot);
        position
    }

    fn check(&self) -> Result<(), OverCap> {
        if self.bytes > self.cap {
            return Err(OverCap {
                queued: self.bytes,
                cap: self.cap,
            });
        }
        Ok(())
    }

    fn retire(&mut self, pane_id: u32) {
        let Some(position) = self.live.remove(&pane_id) else {
            return;
        };
        let Some(index) = position
            .checked_sub(self.head)
            .and_then(|index| usize::try_from(index).ok())
        else {
            return;
        };
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        if let Slot::Delta { frame, .. } = slot {
            self.bytes -= frame.len();
        }
        *slot = Slot::Gone;
        self.gone += 1;
    }

    /// Busy panes behind a stalled socket leave a `Gone` per update; drop them once they are
    /// half the queue, so a stall costs memory per pane, not per update.
    fn compact_if_sparse(&mut self) {
        if self.gone < 32 || self.gone * 2 < self.slots.len() {
            return;
        }
        self.slots.retain(|slot| !matches!(slot, Slot::Gone));
        self.gone = 0;
        self.live.clear();
        for (index, slot) in self.slots.iter().enumerate() {
            if let Slot::Delta { pane_id, .. } | Slot::Snapshot { pane_id } = slot {
                self.live.insert(*pane_id, self.head + index as u64);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, OutQueue, Outgoing, OverCap};

    fn frame(tag: u8, len: usize) -> Frame {
        vec![tag; len].into()
    }

    fn drain(queue: &mut OutQueue) -> Vec<Outgoing> {
        std::iter::from_fn(|| queue.pop()).collect()
    }

    #[test]
    fn controls_keep_their_order_around_live_updates() {
        let mut queue = OutQueue::new(1024);
        queue.push_control(frame(1, 4)).expect("cap");
        queue.push_delta(7, frame(2, 4)).expect("cap");
        queue.push_control(frame(3, 4)).expect("cap");
        assert_eq!(queue.bytes(), 12);
        assert_eq!(
            drain(&mut queue),
            vec![
                Outgoing::Frame(frame(1, 4)),
                Outgoing::Frame(frame(2, 4)),
                Outgoing::Frame(frame(3, 4)),
            ]
        );
        assert_eq!((queue.bytes(), queue.len()), (0, 0));
    }

    #[test]
    fn a_second_delta_turns_the_first_into_a_snapshot_after_the_newest_control() {
        let mut queue = OutQueue::new(1024);
        queue.push_delta(7, frame(2, 100)).expect("cap");
        queue.push_control(frame(1, 4)).expect("cap");
        queue.push_delta(8, frame(4, 10)).expect("cap");
        queue.push_delta(7, frame(3, 100)).expect("cap");
        queue.push_delta(7, frame(5, 100)).expect("cap");
        assert_eq!(queue.bytes(), 14, "the replaced deltas no longer count");
        assert_eq!(
            drain(&mut queue),
            vec![
                Outgoing::Frame(frame(1, 4)),
                Outgoing::Frame(frame(4, 10)),
                Outgoing::Snapshot(7),
            ]
        );
    }

    #[test]
    fn a_resize_snapshot_replaces_a_pending_delta() {
        let mut queue = OutQueue::new(1024);
        queue.push_delta(7, frame(2, 100)).expect("cap");
        queue.push_snapshot(7);
        queue.push_snapshot(7);
        assert_eq!(queue.bytes(), 0);
        assert_eq!(drain(&mut queue), vec![Outgoing::Snapshot(7)]);
    }

    #[test]
    fn after_a_pop_the_pane_starts_over_with_a_plain_delta() {
        let mut queue = OutQueue::new(1024);
        queue.push_delta(7, frame(2, 4)).expect("cap");
        assert_eq!(queue.pop(), Some(Outgoing::Frame(frame(2, 4))));
        queue.push_delta(7, frame(3, 4)).expect("cap");
        assert_eq!(drain(&mut queue), vec![Outgoing::Frame(frame(3, 4))]);
    }

    #[test]
    fn going_over_the_cap_is_reported_and_coalesced_updates_never_reach_it() {
        let mut queue = OutQueue::new(10);
        queue.push_control(frame(1, 6)).expect("under the cap");
        for _ in 0..1000 {
            queue
                .push_delta(7, frame(2, 4))
                .expect("one pending update per pane");
        }
        assert_eq!(
            queue.push_control(frame(3, 5)),
            Err(OverCap {
                queued: 11,
                cap: 10
            })
        );
    }

    #[test]
    fn a_single_frame_larger_than_the_cap_is_over_it() {
        let mut queue = OutQueue::new(10);
        assert!(queue.push_delta(1, frame(1, 11)).is_err());
    }

    #[test]
    fn busy_panes_behind_a_stalled_socket_do_not_grow_the_queue() {
        let mut queue = OutQueue::new(1 << 20);
        queue.push_control(frame(9, 1)).expect("cap");
        for round in 0..10_000u32 {
            queue.push_delta(round % 3, frame(1, 8)).expect("cap");
        }
        assert!(queue.slots.len() < 100, "{} slots", queue.slots.len());
        let mut out = drain(&mut queue);
        assert_eq!(out.remove(0), Outgoing::Frame(frame(9, 1)));
        out.sort_by_key(|item| format!("{item:?}"));
        assert_eq!(
            out,
            vec![
                Outgoing::Snapshot(0),
                Outgoing::Snapshot(1),
                Outgoing::Snapshot(2)
            ]
        );
        assert_eq!(queue.bytes(), 0);
    }

    #[test]
    fn compaction_keeps_positions_valid_after_pops() {
        let mut queue = OutQueue::new(1 << 20);
        for round in 0..200u32 {
            queue.push_control(frame(9, 1)).expect("cap");
            queue.push_delta(round % 2, frame(1, 8)).expect("cap");
            if round % 5 == 0 {
                let _ = queue.pop();
            }
        }
        // Whatever compaction did, each pane has exactly one entry and replacing it works.
        queue.push_delta(0, frame(2, 8)).expect("cap");
        let snapshots = drain(&mut queue)
            .into_iter()
            .filter(|item| !matches!(item, Outgoing::Frame(f) if f[0] == 9))
            .collect::<Vec<_>>();
        assert_eq!(snapshots.len(), 2, "{snapshots:?}");
        assert_eq!(queue.bytes(), 0);
    }
}
