//! Restore GTK's current context after a resource operation or cleanup.

use gtk4::gdk::{self, prelude::*};

pub(super) struct CurrentContext {
    previous: Option<gdk::GLContext>,
    switched: bool,
}

impl CurrentContext {
    pub(super) fn new(owner: &gdk::GLContext) -> Self {
        let previous = gdk::GLContext::current();
        let switched = previous.as_ref() != Some(owner);
        if switched {
            owner.make_current();
        }
        Self { previous, switched }
    }
}

impl Drop for CurrentContext {
    fn drop(&mut self) {
        if !self.switched {
            return;
        }
        match &self.previous {
            Some(context) => context.make_current(),
            None => gdk::GLContext::clear_current(),
        }
    }
}
