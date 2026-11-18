// Present: draws the developed (display-referred linear) photo as a quad inside a RUNE effect
// slot, through the edit's geometry (crop, straighten, rotate, flip): each output position is
// mapped to a source position by one affine map, and positions off the photo show `background`.
// In the split view the part left of the divide shows the photo with no adjustments instead.

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
    // The clipping warnings: linear RGB, and a above 0.5 when the warning is on.
    clip_shadow: vec4<f32>,
    clip_highlight: vec4<f32>,
    // x: where the split view divides the quad (0..1); left of it shows `original`. Below 0
    // when the split is off.
    split: vec4<f32>,
}

// Linear values that encode to 0 and to 255 in 8 bits: the histogram's end bins.
const CLIP_LOW: f32 = 0.000152;
const CLIP_HIGH: f32 = 0.995545;

@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: Present;
@group(0) @binding(3) var original: texture_2d<f32>;

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
    let at = clamp(src, vec2<f32>(0.0), vec2<f32>(1.0));
    let edited = textureSample(photo, smp, at).rgb;
    let before = textureSample(original, smp, at).rgb;
    let sampled = clamp(select(edited, before, in.uv.x < u.split.x), vec3<f32>(0.0), vec3<f32>(1.0));
    let off = any(src < vec2<f32>(-0.0005)) || any(src > vec2<f32>(1.0005));
    var c = sampled;
    // Highlights warn when any channel is at full, shadows only when all three are at zero:
    // a saturated colour has a channel at zero without being crushed.
    if u.clip_highlight.a > 0.5 && any(sampled >= vec3<f32>(CLIP_HIGH)) {
        c = u.clip_highlight.rgb;
    }
    if u.clip_shadow.a > 0.5 && all(sampled < vec3<f32>(CLIP_LOW)) {
        c = u.clip_shadow.rgb;
    }
    c = select(c, u.background.rgb, off);
    if u.encode.x > 0.5 {
        c = srgb_encode(c);
    }
    return vec4<f32>(c, 1.0);
}
