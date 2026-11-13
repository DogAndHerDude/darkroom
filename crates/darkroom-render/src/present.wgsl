// Present: draws the developed (display-referred linear) photo as a quad inside a RUNE effect
// slot, through the edit's geometry (crop, straighten, rotate, flip): each output position is
// mapped to a source position by one affine map, and positions off the photo show `background`.

struct Present {
    // Quad corners in clip space: left, top, right, bottom.
    ndc: vec4<f32>,
    // x: 1 to sRGB-encode the output (the frame blends in sRGB), 0 to write linear values
    // and let an *Srgb target encode them.
    encode: vec4<f32>,
    // Output (0..1 over the quad) → source (0..1): x' = a·x + b·y + e, y' = c·x + d·y + f.
    map_abcd: vec4<f32>,
    map_ef: vec4<f32>,
    // Linear RGB shown where the map leaves the photo (a straightened photo's corners).
    background: vec4<f32>,
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
    let m = u.map_abcd;
    let src = vec2<f32>(m.x * in.uv.x + m.y * in.uv.y + u.map_ef.x, m.z * in.uv.x + m.w * in.uv.y + u.map_ef.y);
    // Sampled outside the branch: textureSample needs uniform control flow.
    let sampled = clamp(textureSample(photo, smp, clamp(src, vec2<f32>(0.0), vec2<f32>(1.0))).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let off = any(src < vec2<f32>(-0.0005)) || any(src > vec2<f32>(1.0005));
    var c = select(sampled, u.background.rgb, off);
    if u.encode.x > 0.5 {
        c = srgb_encode(c);
    }
    return vec4<f32>(c, 1.0);
}
