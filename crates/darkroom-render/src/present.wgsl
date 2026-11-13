// Present: draws the developed (linear, float) photo as a quad inside a RUNE effect slot.

struct Present {
    // Quad corners in clip space: left, top, right, bottom.
    ndc: vec4<f32>,
    // x: 1 to sRGB-encode the output (the frame blends in sRGB), 0 to write linear values
    // and let an *Srgb target encode them.
    encode: vec4<f32>,
}

@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: Present;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    let t = vec2<f32>(f32(i & 1u), f32(i >> 1u));
    var out: VsOut;
    out.pos = vec4<f32>(mix(u.ndc.x, u.ndc.z, t.x), mix(u.ndc.y, u.ndc.w, t.y), 0.0, 1.0);
    out.uv = t;
    return out;
}

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    var c = clamp(textureSample(photo, smp, in.uv).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if u.encode.x > 0.5 {
        c = srgb_encode(c);
    }
    return vec4<f32>(c, 1.0);
}
