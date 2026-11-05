//! Real-driver checks run explicitly under an isolated display, not the user's desktop.

use super::*;
use crate::render::{instances::RectInstance, shaders};
use gtk4::prelude::*;

#[test]
#[ignore = "requires an isolated GTK display: tools/checks/linux-gl-check.sh"]
fn resources_are_released_on_their_owner_and_uploads_are_checked() {
    gtk4::init().expect("GTK display");
    assert!(Gl::load().is_err(), "loading requires a current context");
    let window = gtk4::Window::new();
    gtk4::prelude::WidgetExt::realize(&window);
    let surface = window.surface().expect("surface");
    let owner = surface.create_gl_context().expect("context");
    owner.set_required_version(3, 0);
    owner.realize().expect("realized context");
    owner.make_current();
    let gl = Gl::load().expect("GL loader");
    let second = surface.create_gl_context().expect("second context");
    second.set_required_version(3, 0);
    second.realize().expect("second realized context");
    let [vertex, fragment, _, _] = shaders::sources(shaders::Dialect::detect(&gl.version()));
    for index in 0..32 {
        assert!(pipeline(&gl, &vertex, "not a shader", &[], 32).is_err());
        let pipeline = pipeline(
            &gl,
            &vertex,
            &fragment,
            &[(0, 2, 0), (1, 2, 8), (2, 4, 16)],
            RectInstance::STRIDE,
        )
        .expect("pipeline");
        let texture = Texture::new(&gl, 8, false).expect("texture");
        assert!(texture.upload(0, 0, 2, 2, &[1, 2, 3, 4]));
        assert!(!texture.upload(0, 0, 2, 2, &[1, 2, 3]));
        assert!(!texture.upload(7, 7, 2, 2, &[1, 2, 3, 4]));
        assert!(!texture.upload(-1, 0, 1, 1, &[1]));
        assert!(Texture::new(&gl, 0, false).is_err());
        let names = (texture.name, pipeline.program, pipeline.vbo.expect("vbo"));
        assert_eq!(gl.resources_alive(names), [true; 3]);
        // The GTK rendering context may be absent by the time a window's fields drop.
        gdk::GLContext::clear_current();
        if index % 2 == 0 {
            second.make_current();
        }
        let previous = gdk::GLContext::current();
        drop(texture);
        drop(pipeline);
        assert_eq!(
            gdk::GLContext::current(),
            previous,
            "cleanup restores GTK's previous context"
        );
        assert_eq!(gl.resources_alive(names), [false; 3]);
        assert_eq!(gdk::GLContext::current(), previous);
        owner.make_current();
    }
    gdk::GLContext::clear_current();
    drop(gl);
    window.destroy();
}
