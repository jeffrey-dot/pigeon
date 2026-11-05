//! Shader dialects and sources; no driver calls.

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
    pub fn detect(version: &str) -> Self {
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
