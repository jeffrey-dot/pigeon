/// The renderer's Metal shaders, compiled at runtime with `makeLibrary(source:)`.
///
/// SwiftPM cannot run the `metal` compiler for a plain executable target without Xcode
/// build rules, and the source is small, so it ships as a string. Struct layouts must match
/// `RectInstance` / `GlyphInstance`.
enum MetalShaders {
    static let source = """
    #include <metal_stdlib>
    using namespace metal;

    struct Uniforms {
        float2 viewport;   // drawable size in pixels
    };

    struct RectInstance {
        float2 origin;
        float2 size;
        float4 color;
    };

    struct GlyphInstance {
        float2 origin;
        float2 size;
        float2 uv;
        float colored;
        float padding;
        float4 color;
    };

    struct RectOut {
        float4 position [[position]];
        float4 color;
    };

    struct GlyphOut {
        float4 position [[position]];
        float2 uv;
        float colored;
        float4 color;
    };

    // Pixel space (top-left origin) to clip space.
    static float4 clip(float2 pixel, float2 viewport) {
        float2 ndc = pixel / viewport * 2.0 - 1.0;
        return float4(ndc.x, -ndc.y, 0.0, 1.0);
    }

    // Two triangles per instance, corners from the vertex id: no vertex buffer needed.
    static float2 corner(uint vid) {
        return float2((vid == 1 || vid == 4 || vid == 5) ? 1.0 : 0.0,
                      (vid == 2 || vid == 3 || vid == 5) ? 1.0 : 0.0);
    }

    vertex RectOut rect_vertex(uint vid [[vertex_id]],
                               uint iid [[instance_id]],
                               const device RectInstance *rects [[buffer(0)]],
                               constant Uniforms &u [[buffer(1)]]) {
        RectInstance r = rects[iid];
        RectOut out;
        out.position = clip(r.origin + corner(vid) * r.size, u.viewport);
        out.color = float4(r.color.rgb * r.color.a, r.color.a);
        return out;
    }

    fragment float4 rect_fragment(RectOut in [[stage_in]]) {
        return in.color;
    }

    vertex GlyphOut glyph_vertex(uint vid [[vertex_id]],
                                 uint iid [[instance_id]],
                                 const device GlyphInstance *glyphs [[buffer(0)]],
                                 constant Uniforms &u [[buffer(1)]]) {
        GlyphInstance g = glyphs[iid];
        float2 c = corner(vid);
        GlyphOut out;
        out.position = clip(g.origin + c * g.size, u.viewport);
        out.uv = g.uv + c * g.size;
        out.colored = g.colored;
        out.color = g.color;
        return out;
    }

    // Quads sit on whole pixels and map 1:1 onto atlas texels, so nearest sampling is exact.
    fragment float4 glyph_fragment(GlyphOut in [[stage_in]],
                                   texture2d<float> mask [[texture(0)]],
                                   texture2d<float> color [[texture(1)]]) {
        constexpr sampler s(coord::pixel, filter::nearest, address::clamp_to_zero);
        if (in.colored > 0.5) {
            return color.sample(s, in.uv);   // already premultiplied
        }
        float a = mask.sample(s, in.uv).r * in.color.a;
        return float4(in.color.rgb * a, a);
    }
    """
}
