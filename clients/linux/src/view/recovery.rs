//! Replacing a connection invalidates every UI proof and pending input before new messages.
use super::Workspace;
use crate::host::HostLink;
use gtk4::prelude::*;
use pigeon_core::api::Message;

impl Workspace {
    pub fn reconnected(&mut self, host: HostLink, hello: &Message) {
        let Message::HelloAck {
            host_id,
            instance,
            features,
            ..
        } = hello
        else {
            return;
        };
        let same =
            instance.is_some() && *instance == self.host_instance && *host_id == self.host_identity;
        self.close_search(false);
        self.cancel_input();
        self.input.set_target(None);
        self.host.close();
        self.host = host;
        self.connection_generation = self.connection_generation.wrapping_add(1);
        self.model.reconnect(same, self.host.scrollback());
        self.status.begin_connection(
            host_id,
            instance.as_deref(),
            features.iter().any(|f| f == "status_snapshot"),
        );
        self.host_identity = host_id.clone();
        self.host_instance = instance.clone();
        self.search_supported = features.iter().any(|f| f == "text_query")
            && features.iter().any(|f| f == "text_viewport");
        self.presented.clear();
        self.sized.clear();
        self.sizes.clear();
        self.terminal_focus = None;
        self.wheel.reset();
        self.disconnected_reason.clear();
        self.recovery_notice = if same {
            String::new()
        } else {
            "Host restarted · previous tasks were not restored".into()
        };
        self.set_appearance(crate::theme::dark());
        self.update_input();
        self.update_scroll();
        self.area.queue_render();
    }

    pub fn poll_sessions(&self) {
        if self.model.connected {
            self.host.send(Message::Sessions { items: vec![] });
        }
    }
    pub fn stop_connection(&self) {
        self.host.close();
    }
}
