//! OpenGL resource ownership and checked uploads. All client unsafe stays in this file.

use std::ffi::c_void;
use std::os::raw::c_char;
use std::rc::Rc;

use super::current_context::CurrentContext;
pub use super::device::Gl;
use glow::HasContext;
use gtk4::gdk;
use libloading::Library;

use super::instances::Instance;
pub use super::pipeline::Pipeline;
pub use super::texture::Texture;

impl Gl {
    pub fn load() -> Result<Rc<Self>, String> {
        let owner = gdk::GLContext::current().ok_or("no current GDK GL context")?;
        // SAFETY: driver libraries stay owned alongside every loaded function pointer.
        let egl = unsafe { Library::new("libEGL.so.1") }.ok();
        // SAFETY: the GL library is kept for the same lifetime as the glow context.
        let gl_lib = unsafe { Library::new("libGL.so.1") }.ok();
        let egl_proc: Option<unsafe extern "C" fn(*const c_char) -> *const c_void> =
            egl.as_ref().and_then(|lib| {
                // SAFETY: the documented EGL symbol has this signature; the library is live.
                unsafe { lib.get(b"eglGetProcAddress\0") }
                    .ok()
                    .map(|symbol| *symbol)
            });
        // SAFETY: a realized GDK context is current; symbols come from retained GL drivers.
        let context = unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                if let Some(resolve) = egl_proc {
                    let pointer = resolve(name.as_ptr());
                    if !pointer.is_null() {
                        return pointer;
                    }
                }
                for lib in [gl_lib.as_ref(), egl.as_ref()].into_iter().flatten() {
                    if let Ok(symbol) = lib.get::<*const c_void>(name.to_bytes_with_nul()) {
                        if !(*symbol).is_null() {
                            return *symbol;
                        }
                    }
                }
                std::ptr::null()
            })
        };
        Ok(Rc::new(Self {
            context,
            owner,
            _egl: egl,
            _gl: gl_lib,
        }))
    }

    fn current(&self) -> CurrentContext {
        CurrentContext::new(&self.owner)
    }

    pub fn version(&self) -> String {
        let _current = self.current();
        // SAFETY: this driver belongs to the current, retained context.
        unsafe { self.context.get_parameter_string(glow::VERSION) }
    }

    pub fn begin(&self, viewport: (i32, i32), clear: [f32; 4]) {
        let _current = self.current();
        // SAFETY: state changes use the owning context; no client pointers cross GL.
        unsafe {
            self.context.viewport(0, 0, viewport.0, viewport.1);
            self.context.disable(glow::SCISSOR_TEST);
            self.context
                .clear_color(clear[0], clear[1], clear[2], clear[3]);
            self.context.clear(glow::COLOR_BUFFER_BIT);
            self.context.enable(glow::BLEND);
            self.context.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
        }
    }

    pub fn scissor(&self, rect: Option<[i32; 4]>) {
        let _current = self.current();
        // SAFETY: dimensions are values, and the owning context is current.
        unsafe {
            if let Some([x, y, width, height]) = rect {
                self.context.enable(glow::SCISSOR_TEST);
                self.context.scissor(x, y, width.max(0), height.max(0));
            } else {
                self.context.disable(glow::SCISSOR_TEST);
            }
        }
    }

    pub fn finish(&self) {
        let _current = self.current();
        // SAFETY: detach resources from the current context before GTK composes its frame.
        unsafe {
            self.context.disable(glow::SCISSOR_TEST);
            for unit in [glow::TEXTURE1, glow::TEXTURE0] {
                self.context.active_texture(unit);
                self.context.bind_texture(glow::TEXTURE_2D, None);
            }
            self.context.use_program(None);
        }
    }
}

impl Texture {
    pub fn new(gl: &Rc<Gl>, side: i32, colored: bool) -> Result<Self, String> {
        let _current = gl.current();
        let context = &gl.context;
        // SAFETY: resource creation uses the owning current context and value-only sizes.
        unsafe {
            if side <= 0 || side > context.get_parameter_i32(glow::MAX_TEXTURE_SIZE) {
                return Err("atlas size exceeds driver limit".into());
            }
            let name = context
                .create_texture()
                .map_err(|error| format!("texture: {error}"))?;
            let texture = Self {
                gl: Rc::clone(gl),
                name,
                side,
                colored,
            };
            context.bind_texture(glow::TEXTURE_2D, Some(name));
            for parameter in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                context.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::LINEAR as i32);
            }
            for parameter in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
                context.tex_parameter_i32(glow::TEXTURE_2D, parameter, glow::CLAMP_TO_EDGE as i32);
            }
            context.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                if colored { glow::RGBA8 } else { glow::R8 } as i32,
                side,
                side,
                0,
                texture.format(),
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            context.bind_texture(glow::TEXTURE_2D, None);
            if context.get_error() != glow::NO_ERROR {
                return Err("atlas allocation failed".into());
            }
            Ok(texture)
        }
    }

    fn format(&self) -> u32 {
        if self.colored {
            glow::RGBA
        } else {
            glow::RED
        }
    }

    pub fn upload(&self, x: i32, y: i32, width: i32, height: i32, bytes: &[u8]) -> bool {
        if x < 0
            || y < 0
            || width <= 0
            || height <= 0
            || x.checked_add(width).is_none_or(|right| right > self.side)
            || y.checked_add(height)
                .is_none_or(|bottom| bottom > self.side)
            || (width as usize)
                .checked_mul(height as usize)
                .and_then(|area| area.checked_mul(if self.colored { 4 } else { 1 }))
                != Some(bytes.len())
        {
            return false;
        }
        let _current = self.gl.current();
        // SAFETY: bounds and exact byte length were checked, GL copies the live slice synchronously.
        unsafe {
            let gl = &self.gl.context;
            gl.bind_texture(glow::TEXTURE_2D, Some(self.name));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                x,
                y,
                width,
                height,
                self.format(),
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(bytes)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        true
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        let _current = self.gl.current();
        // SAFETY: unique texture ownership; its context and loader remain live through this drop.
        unsafe {
            self.gl.context.delete_texture(self.name);
        }
    }
}

pub fn pipeline(
    gl: &Rc<Gl>,
    vertex: &str,
    fragment: &str,
    attributes: &[(u32, i32, i32)],
    stride: i32,
) -> Result<Pipeline, String> {
    let _current = gl.current();
    let context = &gl.context;
    // SAFETY: all created handles stay owned by result, including on early errors.
    unsafe {
        let program = link(context, vertex, fragment)?;
        let mut result = Pipeline {
            gl: Rc::clone(gl),
            program,
            viewport: context.get_uniform_location(program, "u_viewport"),
            mask: context.get_uniform_location(program, "u_mask"),
            colour: context.get_uniform_location(program, "u_color"),
            kind: context.get_uniform_location(program, "u_kind"),
            vao: None,
            vbo: None,
        };
        result.vao = Some(
            context
                .create_vertex_array()
                .map_err(|error| format!("vao: {error}"))?,
        );
        result.vbo = Some(
            context
                .create_buffer()
                .map_err(|error| format!("vbo: {error}"))?,
        );
        context.bind_vertex_array(result.vao);
        context.bind_buffer(glow::ARRAY_BUFFER, result.vbo);
        for (location, components, offset) in attributes {
            context.enable_vertex_attrib_array(*location);
            context.vertex_attrib_pointer_f32(
                *location,
                *components,
                glow::FLOAT,
                false,
                stride,
                *offset,
            );
            context.vertex_attrib_divisor(*location, 1);
        }
        context.bind_vertex_array(None);
        context.bind_buffer(glow::ARRAY_BUFFER, None);
        Ok(result)
    }
}

impl Pipeline {
    pub fn draw<T: Instance>(&self, items: &[T], viewport: (i32, i32)) {
        if items.is_empty() || items.len() > i32::MAX as usize {
            return;
        }
        let _current = self.gl.current();
        // SAFETY: sealed repr(C) layouts consist solely of initialized f32 arrays, without padding.
        let bytes = unsafe {
            std::slice::from_raw_parts(items.as_ptr().cast::<u8>(), std::mem::size_of_val(items))
        };
        // SAFETY: all handles belong to the current context; the upload copies the live byte slice.
        unsafe {
            let gl = &self.gl.context;
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.viewport.as_ref(), viewport.0 as f32, viewport.1 as f32);
            gl.bind_vertex_array(self.vao);
            gl.bind_buffer(glow::ARRAY_BUFFER, self.vbo);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
            gl.draw_arrays_instanced(glow::TRIANGLES, 0, 6, items.len() as i32);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    pub fn bind_atlases(&self, mask: &Texture, color: &Texture) {
        if !Rc::ptr_eq(&self.gl, &mask.gl) || !Rc::ptr_eq(&self.gl, &color.gl) {
            return;
        }
        let _current = self.gl.current();
        // SAFETY: pipeline and textures share this current context.
        unsafe {
            let gl = &self.gl.context;
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(mask.name));
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(color.name));
            gl.use_program(Some(self.program));
            gl.uniform_1_i32(self.mask.as_ref(), 0);
            gl.uniform_1_i32(self.colour.as_ref(), 1);
        }
    }

    pub fn set_kind(&self, kind: i32) {
        let _current = self.gl.current();
        // SAFETY: uniforms and blend state refer to the owning current context.
        unsafe {
            self.gl.context.use_program(Some(self.program));
            self.gl.context.uniform_1_i32(self.kind.as_ref(), kind);
            self.gl.context.blend_func_separate(
                if kind == 1 {
                    glow::ONE
                } else {
                    glow::SRC_ALPHA
                },
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _current = self.gl.current();
        // SAFETY: these unique handles were created by this retained context.
        unsafe {
            let gl = &self.gl.context;
            gl.use_program(None);
            if let Some(vbo) = self.vbo {
                gl.delete_buffer(vbo);
            }
            if let Some(vao) = self.vao {
                gl.delete_vertex_array(vao);
            }
            gl.delete_program(self.program);
        }
    }
}

unsafe fn link(gl: &glow::Context, vertex: &str, fragment: &str) -> Result<glow::Program, String> {
    let program = gl
        .create_program()
        .map_err(|error| format!("program: {error}"))?;
    let mut shaders = Vec::new();
    let result = (|| {
        for (kind, source) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = gl
                .create_shader(kind)
                .map_err(|error| format!("shader: {error}"))?;
            shaders.push(shader);
            gl.attach_shader(program, shader);
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(format!(
                    "shader does not compile: {}",
                    gl.get_shader_info_log(shader)
                ));
            }
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(format!(
                "program does not link: {}",
                gl.get_program_info_log(program)
            ));
        }
        Ok(program)
    })();
    for shader in shaders {
        gl.detach_shader(program, shader);
        gl.delete_shader(shader);
    }
    if result.is_err() {
        gl.delete_program(program);
    }
    result
}

#[cfg(test)]
#[path = "gl_tests.rs"]
mod tests;
