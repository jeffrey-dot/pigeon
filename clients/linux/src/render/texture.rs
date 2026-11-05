//! Owned GL resource metadata; operations and destruction stay in gl.rs.

use super::gl::Gl;
use std::rc::Rc;

pub struct Texture {
    pub(super) gl: Rc<Gl>,
    pub(super) name: glow::Texture,
    pub(super) side: i32,
    pub(super) colored: bool,
}
