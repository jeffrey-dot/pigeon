//! Owned GL resource metadata; operations and destruction stay in gl.rs.

use super::gl::Gl;
use std::rc::Rc;

pub struct Pipeline {
    pub(super) gl: Rc<Gl>,
    pub(super) program: glow::Program,
    pub(super) viewport: Option<glow::UniformLocation>,
    pub(super) mask: Option<glow::UniformLocation>,
    pub(super) colour: Option<glow::UniformLocation>,
    pub(super) kind: Option<glow::UniformLocation>,
    pub(super) vao: Option<glow::VertexArray>,
    pub(super) vbo: Option<glow::Buffer>,
}
