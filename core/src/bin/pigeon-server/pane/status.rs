//! Broadcasts one collection mutation in both negotiated wire shapes.

use pigeon_core::{proto, session::PaneState};
use tokio::sync::broadcast;

use crate::host::event::{encode, Event};

#[derive(Default)]
pub struct StatusBroadcast {
    revision: u64,
}

impl StatusBroadcast {
    pub fn publish(&mut self, pane: &PaneState, events: &broadcast::Sender<Event>) {
        let revision = pane.status().revision();
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let snapshot = proto::status_snapshot_message(pane.pane_id, pane.status().snapshot());
        if let (Some(legacy), Some(snapshot)) = (encode(&pane.status_message()), encode(&snapshot))
        {
            let _ = events.send(Event::Status {
                pane_id: pane.pane_id,
                legacy,
                snapshot,
            });
        }
    }
}
