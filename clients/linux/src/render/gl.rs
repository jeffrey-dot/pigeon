//! The OpenGL 3.3 core path: function loading, shaders, instance buffers.
//!
//! This is the only `unsafe` in the Linux client (AGENTS.md §3): the GL boundary itself.
//! Everything above it works through glow's checked wrappers, and the shaders are strings
//! for the same reason macOS compiles MSL at runtime: no build rule should be needed to
//! draw a grid (`docs/renderer.md` §5.1).

use std::ffi::c_void;
use std::os::raw::c_char;

use glow::HasContext;
use libloading::Library;

const RECT_VERTEX: &str = r#"
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_size;
layout(location = 2) in vec4 a_color;
uniform vec2 u_viewport;
out vec4 v_color;
const vec2 CORNERS[6] = vec2[6](
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
    vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
void main() {
    vec2 px = a_pos + CORNERS[gl_VertexID] * a_size;
    gl_Position = vec4(px.x / u_viewport.x * 2.0 - 1.0,
                       1.0 - px.y / u_viewport.y * 2.0, 0.0, 1.0);
    v_color = a_color;
}
"#;

const RECT_FRAGMENT: &str = r#"
in vec4 v_color;
out vec4 frag;
void main() { frag = v_color; }
"#;

const GLYPH_VERTEX: &str = r#"
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_size;
layout(location = 2) in vec4 a_uv;
layout(location = 3) in vec4 a_color;
uniform vec2 u_viewport;
out vec2 v_uv;
out vec4 v_color;
const vec2 CORNERS[6] = vec2[6](
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
    vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
void main() {
    vec2 corner = CORNERS[gl_VertexID];
    vec2 px = a_pos + corner * a_size;
    gl_Position = vec4(px.x / u_viewport.x * 2.0 - 1.0,
                       1.0 - px.y / u_viewport.y * 2.0, 0.0, 1.0);
    v_uv = a_uv.xy + corner * a_uv.zw;
    v_color = a_color;
}
"#;

const GLYPH_FRAGMENT: &str = r#"
in vec2 v_uv;
in vec4 v_color;
uniform sampler2D u_mask;
uniform sampler2D u_color;
// 0 = 8-bit coverage tinted by v_color, 1 = premultiplied RGBA drawn as is (emoji).
uniform int u_kind;
out vec4 frag;
void main() {
    if (u_kind == 1) {
        vec4 colour = texture(u_color, v_uv);
        frag = vec4(colour.rgb, colour.a * v_color.a);
    } else {
        float coverage = texture(u_mask, v_uv).r;
        frag = vec4(v_color.rgb, v_color.a * coverage);
    }
}
"#;

/// A loaded GL context plus the libraries its entry points came from.
pub struct Gl {
    pub context: glow::Context,
    // Dropping these would leave the pointers above dangling.
    _egl: Option<Library>,
    _gl: Option<Library>,
}

impl Gl {
    /// Must be called with the GLArea's context current.
    pub fn load() -> Result<Self, String> {
        // GDK uses EGL on Wayland and may use GLX on X11; ask EGL first, then the driver
        // libraries, so both window systems resolve.
        let egl = unsafe { Library::new("libEGL.so.1") }.ok();
        let gl_lib = unsafe { Library::new("libGL.so.1") }.ok();
        let egl_proc: Option<unsafe extern "C" fn(*const c_char) -> *const c_void> =
            egl.as_ref().and_then(|lib| {
                unsafe { lib.get(b"eglGetProcAddress\0") }
                    .ok()
                    .map(|symbol| *symbol)
            });

        let context = unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                if let Some(egl_proc) = egl_proc {
                    let pointer = egl_proc(name.as_ptr());
                    if !pointer.is_null() {
                        return pointer;
                    }
                }
                for lib in [gl_lib.as_ref(), egl.as_ref()].into_iter().flatten() {
                    if let Ok(symbol) = lib.get::<*const c_void>(name.to_bytes_with_nul()) {
                        let pointer = *symbol;
                        if !pointer.is_null() {
                            return pointer;
                        }
                    }
                }
                std::ptr::null()
            })
        };

        Ok(Self {
            context,
            _egl: egl,
            _gl: gl_lib,
        })
    }

    pub fn version(&self) -> String {
        unsafe { self.context.get_parameter_string(glow::VERSION) }
    }
}

/// One shader program, its instance buffer and its VAO.
pub struct Pipeline {
    program: glow::Program,
    viewport: Option<glow::UniformLocation>,
    mask: Option<glow::UniformLocation>,
    colour: Option<glow::UniformLocation>,
    kind: Option<glow::UniformLocation>,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
}

/// `attributes` are `(location, components, offset)` for the per-instance layout.
pub fn pipeline(
    gl: &glow::Context,
    vertex: &str,
    fragment: &str,
    attributes: &[(u32, i32, i32)],
    stride: i32,
) -> Result<Pipeline, String> {
    unsafe {
        let program = link(gl, vertex, fragment)?;
        let vao = gl
            .create_vertex_array()
            .map_err(|error| format!("vao: {error}"))?;
        let vbo = gl
            .create_buffer()
            .map_err(|error| format!("vbo: {error}"))?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        for (location, components, offset) in attributes {
            gl.enable_vertex_attrib_array(*location);
            gl.vertex_attrib_pointer_f32(
                *location,
                *components,
                glow::FLOAT,
                false,
                stride,
                *offset,
            );
            gl.vertex_attrib_divisor(*location, 1);
        }
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        Ok(Pipeline {
            program,
            viewport: gl.get_uniform_location(program, "u_viewport"),
            mask: gl.get_uniform_location(program, "u_mask"),
            colour: gl.get_uniform_location(program, "u_color"),
            kind: gl.get_uniform_location(program, "u_kind"),
            vao,
            vbo,
        })
    }
}

impl Pipeline {
    /// Uploads `items` and issues one instanced draw of six vertices per item.
    pub fn draw<T>(&self, gl: &glow::Context, items: &[T], viewport: (i32, i32)) {
        if items.is_empty() {
            return;
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(items.as_ptr().cast::<u8>(), std::mem::size_of_val(items))
        };
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.viewport.as_ref(), viewport.0 as f32, viewport.1 as f32);
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
            gl.draw_arrays_instanced(glow::TRIANGLES, 0, 6, items.len() as i32);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    /// Points the glyph shader at the two atlases (units 0 and 1).
    pub fn bind_atlases(&self, gl: &glow::Context) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_1_i32(self.mask.as_ref(), 0);
            gl.uniform_1_i32(self.colour.as_ref(), 1);
        }
    }

    /// `0` = coverage mask, `1` = premultiplied colour (`atlas::Layer`).
    pub fn set_kind(&self, gl: &glow::Context, kind: i32) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_1_i32(self.kind.as_ref(), kind);
        }
    }
}

unsafe fn link(gl: &glow::Context, vertex: &str, fragment: &str) -> Result<glow::Program, String> {
    let program = gl
        .create_program()
        .map_err(|error| format!("program: {error}"))?;
    let mut shaders = Vec::new();
    for (kind, source) in [
        (glow::VERTEX_SHADER, vertex),
        (glow::FRAGMENT_SHADER, fragment),
    ] {
        let shader = match gl.create_shader(kind) {
            Ok(shader) => shader,
            Err(error) => {
                gl.delete_program(program);
                return Err(format!("shader: {error}"));
            }
        };
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let log = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            gl.delete_program(program);
            return Err(format!("shader does not compile: {log}"));
        }
        gl.attach_shader(program, shader);
        shaders.push(shader);
    }
    gl.link_program(program);
    let linked = gl.get_program_link_status(program);
    for shader in shaders {
        gl.detach_shader(program, shader);
        gl.delete_shader(shader);
    }
    if !linked {
        let log = gl.get_program_info_log(program);
        gl.delete_program(program);
        return Err(format!("program does not link: {log}"));
    }
    Ok(program)
}

/// Which GLSL dialect the context speaks.
///
/// GDK hands out an OpenGL ES context through EGL on Wayland (and can do so on X11 too),
/// while a GLX desktop context is plain GL. Same shaders, two headers: ES has no 3.30 and
/// GLSL 3.30 has no `precision` statement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialect {
    Embedded,
    Desktop,
}

impl Dialect {
    pub fn detect(gl: &glow::Context) -> Self {
        let version = unsafe { gl.get_parameter_string(glow::VERSION) };
        if version.contains("OpenGL ES") {
            Self::Embedded
        } else {
            Self::Desktop
        }
    }

    fn header(self) -> &'static str {
        match self {
            Self::Embedded => "#version 300 es\nprecision highp float;\n",
            Self::Desktop => "#version 330 core\n",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Embedded => "OpenGL ES 3",
            Self::Desktop => "OpenGL 3.3",
        }
    }
}

/// The four shader sources for this context's dialect.
pub fn sources(dialect: Dialect) -> [String; 4] {
    let header = dialect.header();
    [
        format!("{header}{RECT_VERTEX}"),
        format!("{header}{RECT_FRAGMENT}"),
        format!("{header}{GLYPH_VERTEX}"),
        format!("{header}{GLYPH_FRAGMENT}"),
    ]
}
