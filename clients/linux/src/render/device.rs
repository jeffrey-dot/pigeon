//! Context and loader ownership shared by GL resources on GTK's main thread.

use gtk4::gdk;
use libloading::Library;

pub struct Gl {
    pub(super) context: glow::Context,
    pub(super) owner: gdk::GLContext,
    // Libraries must outlive every resource's loaded function pointer.
    pub(super) _egl: Option<Library>,
    pub(super) _gl: Option<Library>,
}

impl Gl {
    pub fn owns_current_context(&self) -> bool {
        gdk::GLContext::current().as_ref() == Some(&self.owner)
    }
}
