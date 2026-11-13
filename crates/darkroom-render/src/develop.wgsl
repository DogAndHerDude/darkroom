// Develop, first pass: the photo through the per-pixel steps of the spec's fixed order, into the
// stage texture (linear Rec.2020, unbounded) the detail passes read.
//
//   source (sRGB) → linear Rec.2020 working space
//   1 white balance → 2 exposure → 3 light (black point, then tone) → 4 color → 5 curves
//
// Then: noise reduction (denoise.wgsl), the definition blur (pyramid.wgsl), and finish.wgsl for
// sharpening, definition, vignette and the output transform.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var stage: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> p: Params;
// Four curves of LUT_SIZE samples each: composite RGB, red, green, blue.
@group(0) @binding(3) var<storage, read> curves: array<f32>;
// The curve point pickers' probe result (see `probe`).
@group(0) @binding(4) var<storage, read_write> probed: array<f32, 4>;

const LUT_SIZE: u32 = 1024u;

// ---- 3 light --------------------------------------------------------------------------------

// Global tone on encoded luminance `e`. Weights are taken at min(e, 1) so values past white
// only shift, never fold back.
fn tone(e_in: f32) -> f32 {
    var e = e_in;
    let brilliance = p.light.z;
    let highlights = p.light.w + brilliance * -0.6;
    let shadows = p.tone.x + brilliance * 0.6;
    let brightness = p.tone.y;
    let contrast = p.tone.z + brilliance * 0.2;

    var q = clamp(e, 0.0, 1.0);
    e = e + highlights * 1.5 * q * q * (1.0 - q);           // peaks around 2/3
    q = clamp(e, 0.0, 1.0);
    e = e + shadows * 1.5 * q * (1.0 - q) * (1.0 - q);      // peaks around 1/3
    q = clamp(e, 0.0, 1.0);
    e = e + brightness * 0.8 * q * (1.0 - q);               // midtones
    q = clamp(e, 0.0, 1.0);
    e = e + contrast * (q - 0.5) * 4.0 * q * (1.0 - q);     // S around middle grey, ends fixed
    return max(e, 0.0);
}

fn light(c_in: vec3<f32>) -> vec3<f32> {
    // Black point: positive crushes, negative lifts, on each channel in linear light.
    let b = p.light.y * 0.03;
    let c = max((c_in - vec3<f32>(b)) / (1.0 - b), vec3<f32>(0.0));

    if all(p.tone == vec4<f32>(0.0)) && p.light.z == 0.0 && p.light.w == 0.0 {
        return c;
    }
    // Tone works on luminance and scales the channels by the ratio, so hue holds.
    let y = dot(c, LUMA);
    return with_luma(c, y, decode(tone(encode(y))));
}

// ---- 4 color --------------------------------------------------------------------------------

fn color(c: vec3<f32>) -> vec3<f32> {
    if p.color.x == 0.0 && p.color.y == 0.0 {
        return c;
    }
    var lab = srgb_to_oklab(rec2020_to_srgb(c));
    let chroma = length(lab.yz);
    // Vibrance: chroma raised (or lowered) mostly in muted colours, barely in vivid ones.
    let vib = p.color.y * (1.0 - clamp(chroma / 0.25, 0.0, 1.0));
    lab = vec3<f32>(lab.x, lab.yz * max(1.0 + vib, 0.0) * (1.0 + p.color.x));
    return srgb_to_2020(oklab_to_srgb(lab));
}

// ---- 5 curves -------------------------------------------------------------------------------

fn lut(curve: u32, e: f32) -> f32 {
    if e >= 1.0 {
        return curves[curve * LUT_SIZE + LUT_SIZE - 1u] + (e - 1.0);
    }
    let x = max(e, 0.0) * f32(LUT_SIZE - 1u);
    let i = u32(floor(x));
    let f = x - f32(i);
    let a = curves[curve * LUT_SIZE + i];
    let b = curves[curve * LUT_SIZE + min(i + 1u, LUT_SIZE - 1u)];
    return mix(a, b, f);
}

// Curves act on the display's red, green and blue (sRGB primaries), which is what R, G and B
// mean to someone looking at the screen; on Rec.2020 channels they would shift hues.
fn apply_curves(c2020: vec3<f32>) -> vec3<f32> {
    var e = encode3(rec2020_to_srgb(c2020));
    e = vec3<f32>(lut(0u, e.r), lut(0u, e.g), lut(0u, e.b));
    e = vec3<f32>(lut(1u, e.r), lut(2u, e.g), lut(3u, e.b));
    return srgb_to_2020(decode3(e));
}

// Steps 1–4: everything before curves.
fn before_curves(at: vec2<u32>) -> vec3<f32> {
    // The source is an *Srgb texture, so this load is already linear.
    var c = srgb_to_2020(textureLoad(src, vec2<i32>(at), 0).rgb);
    c = c * p.white_balance.rgb;                // 1
    c = c * p.light.x;                          // 2
    c = light(c);                               // 3
    return color(c);                            // 4
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(src);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    var c = before_curves(id.xy);
    if p.effects.y > 0.5 { c = apply_curves(c); } // 5
    textureStore(stage, vec2<i32>(id.xy), vec4<f32>(c, 1.0));
}

// The curve point pickers' probe: one pixel's value as the curves receive it (display red, green
// and blue in the curves' encoding), at the pixel `p.effects.zw`.
@compute @workgroup_size(1)
fn probe() {
    let size = textureDimensions(src);
    let at = min(vec2<u32>(p.effects.zw), size - vec2<u32>(1u));
    let e = encode3(rec2020_to_srgb(before_curves(at)));
    probed[0] = e.r;
    probed[1] = e.g;
    probed[2] = e.b;
    probed[3] = 1.0;
}
