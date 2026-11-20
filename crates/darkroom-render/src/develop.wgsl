// Develop, first pass: the photo through the per-pixel steps of the spec's fixed order, into the
// stage texture (linear Rec.2020, unbounded) the detail passes read.
//
//   source (sRGB) → linear Rec.2020 working space
//   1 white balance → 2 exposure → 3 light (black point, then tone) → 4 color (saturation and
//   vibrance, Selective Color, black and white) → 5 curves
//
// Before it, when Brilliance, Highlights or Shadows are set: the edge-aware base of the photo's
// luminance (base.wgsl), which makes those three act on regions. After it: noise reduction
// (denoise.wgsl), the base again for Definition, and finish.wgsl for sharpening, definition,
// vignette and the output transform.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var stage: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> p: Params;
// Four curves of LUT_SIZE samples each: composite RGB, red, green, blue.
@group(0) @binding(3) var<storage, read> curves: array<f32>;
// The curve point pickers' probe result (see `probe`).
@group(0) @binding(4) var<storage, read_write> probed: array<f32, 4>;
// The base's a and b at quarter size (base.wgsl), of the luminance the tone step receives.
@group(0) @binding(5) var tone_base: texture_2d<f32>;
@group(0) @binding(6) var smp: sampler;

const LUT_SIZE: u32 = 1024u;

// ---- 3 light --------------------------------------------------------------------------------

// The curve through (0, 0) and (1, 1) with slope `k` at 1: a parabola, with a finite slope at 0
// so noise near the end isn't stretched. It stays monotonic for k up to 2.
fn ease(u: f32, k_in: f32) -> f32 {
    let k = min(k_in, 2.0);
    return u * ((2.0 - k) + (k - 1.0) * u);
}

// Where a pixel `e` (0..1) goes when its region's base moves from `b` to `to`. A pixel at the
// base goes with it, and one near the base keeps its distance from it (the slope there is 1),
// so the region's detail keeps its contrast. Further away the pixel eases back to the ends:
// black stays black, white stays white, and nothing can clip. A base moved more than halfway
// to an end leaves too little room on that side, and detail there is compressed after all.
fn follow(e: f32, b: f32, to_in: f32) -> f32 {
    let to = clamp(to_in, 0.005, 0.995);
    if e <= b {
        return to * ease(e / b, b / to);
    }
    return 1.0 - (1.0 - to) * ease((1.0 - e) / (1.0 - b), (1.0 - b) / (1.0 - to));
}

// Highlights and Shadows (and Brilliance, which moves both) on encoded luminance `e`, by the
// brightness of the region the pixel is in, `base`, rather than its own. The base moves along
// the curves b + 1.5·h·b²(1 − b) and b + 1.5·s·b(1 − b)², bumps peaking around 2/3 and 1/3, and
// the pixel follows it. So a dark region lifts as a whole and keeps the contrast it had, where
// one tone curve for the whole photo would flatten its midtones.
fn regional(e_in: f32, base: f32) -> f32 {
    // Values past white are left alone, so they only shift, never fold back.
    if e_in >= 1.0 {
        return e_in;
    }
    let brilliance = p.light.z;
    let highlights = p.light.w + brilliance * -0.6;
    let shadows = p.tone.x + brilliance * 0.6;
    var e = e_in;
    // Kept off the ends, where `follow` would divide by zero.
    var b = clamp(base, 0.02, 0.98);
    if highlights != 0.0 {
        let to = b + 1.5 * highlights * b * b * (1.0 - b);
        e = follow(e, b, to);
        b = clamp(to, 0.02, 0.98);
    }
    if shadows != 0.0 {
        e = follow(e, b, b + 1.5 * shadows * b * (1.0 - b) * (1.0 - b));
    }
    return e;
}

// Brightness and Contrast: global, on the pixel's own encoded luminance. Weights are taken at
// min(e, 1) so values past white only shift, never fold back.
fn tone(e_in: f32) -> f32 {
    var e = e_in;
    let brightness = p.tone.y;
    let contrast = p.tone.z + p.light.z * 0.2;

    var q = clamp(e, 0.0, 1.0);
    e = e + brightness * 0.8 * q * (1.0 - q);               // midtones
    q = clamp(e, 0.0, 1.0);
    e = e + contrast * (q - 0.5) * 4.0 * q * (1.0 - q);     // S around middle grey, ends fixed
    return max(e, 0.0);
}

// `c` is the photo after the black point; `uv` is where the pixel is, for its region's base.
fn light(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    if all(p.tone == vec4<f32>(0.0)) && p.light.z == 0.0 && p.light.w == 0.0 {
        return c;
    }
    // Tone works on luminance and scales the channels by the ratio, so hue holds.
    let y = dot(c, LUMA);
    var e = encode(y);
    if p.light.z != 0.0 || p.light.w != 0.0 || p.tone.x != 0.0 {
        let ab = textureSampleLevel(tone_base, smp, uv, 0.0).rg;
        e = regional(e, ab.x * e + ab.y);
    }
    return with_luma(c, y, decode(tone(e)));
}

// ---- 4 color --------------------------------------------------------------------------------

const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;
// Oklab chroma at which a colour feels the bands fully; below it they fade out towards grey.
// Must match `bands::GREY_CHROMA` (a test in params.rs checks).
const GREY_CHROMA: f32 = 0.04;

fn band(v: array<vec4<f32>, 2>, i: u32) -> f32 {
    return v[i >> 2u][i & 3u];
}

// The two bands either side of hue `h` (radians) and the weight of the second, as bands::pair:
// a raised cosine from one centre to the next, so the weights are smooth and sum to 1.
fn band_pair(h: f32) -> vec3<f32> {
    let hue = h - TAU * floor(h / TAU);
    for (var i = 0u; i < 8u; i++) {
        let j = (i + 1u) % 8u;
        let c = band(p.band_centres, i);
        var gap = band(p.band_centres, j) - c;
        gap -= TAU * floor(gap / TAU);
        var along = hue - c;
        along -= TAU * floor(along / TAU);
        if along < gap {
            return vec3<f32>(f32(i), f32(j), 0.5 - 0.5 * cos(PI * along / gap));
        }
    }
    return vec3<f32>(0.0, 1.0, 0.0);
}

// One per-band setting at a hue, from the two bands either side of it.
fn banded(v: array<vec4<f32>, 2>, pair: vec3<f32>) -> f32 {
    return mix(band(v, u32(pair.x)), band(v, u32(pair.y)), pair.z);
}

// How much a colour of chroma `c` feels the bands: none at grey, so noise in a grey wall
// doesn't pick up colour.
fn presence(c: f32) -> f32 {
    return smoothstep(0.0, GREY_CHROMA, c);
}

// Selective Color: each band's hue turned, chroma scaled and lightness moved, at constant
// everything else.
fn selective(lab: vec3<f32>) -> vec3<f32> {
    let chroma = length(lab.yz);
    let k = presence(chroma);
    if k == 0.0 {
        return lab;
    }
    let h = atan2(lab.z, lab.y);
    let pair = band_pair(h);
    let turned = h + banded(p.band_hue, pair) * k;
    let c = chroma * max(1.0 + banded(p.band_saturation, pair) * k, 0.0);
    let l = lab.x * exp2(banded(p.band_luminance, pair) * k);
    return vec3<f32>(l, c * cos(turned), c * sin(turned));
}

// Black and white: the colour's own lightness, moved by its bands' mixer settings, with no
// chroma. Grey is left at its lightness whatever the mixer says.
fn monochrome(lab: vec3<f32>) -> vec3<f32> {
    let chroma = length(lab.yz);
    var l = lab.x;
    let k = presence(chroma);
    if k > 0.0 {
        l *= exp2(banded(p.band_mix, band_pair(atan2(lab.z, lab.y))) * k);
    }
    return vec3<f32>(l, 0.0, 0.0);
}

fn color(c: vec3<f32>) -> vec3<f32> {
    if all(p.color == vec4<f32>(0.0)) {
        return c;
    }
    var lab = srgb_to_oklab(rec2020_to_srgb(c));
    let chroma = length(lab.yz);
    // Vibrance: chroma raised (or lowered) mostly in muted colours, barely in vivid ones.
    let vib = p.color.y * (1.0 - clamp(chroma / 0.25, 0.0, 1.0));
    lab = vec3<f32>(lab.x, lab.yz * max(1.0 + vib, 0.0) * (1.0 + p.color.x));
    if p.color.z > 0.5 { lab = selective(lab); }
    if p.color.w > 0.5 {
        // Grey of Oklab lightness L is L³ in every channel of any RGB with a D65 white; written
        // straight out, so the channels are equal exactly, not to within the matrices' rounding.
        let l = monochrome(lab).x;
        return vec3<f32>(l * l * l);
    }
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
// mean to someone looking at the screen; on Rec.2020 channels they would shift hues. In the
// luminance mode the composite curve maps encoded luminance instead and the channels are scaled
// by the ratio, so a contrast curve leaves saturation and hue as they were.
fn apply_curves(c2020: vec3<f32>, luminance: bool) -> vec3<f32> {
    var c = c2020;
    if luminance {
        let y = dot(c, LUMA);
        c = with_luma(c, y, decode(lut(0u, encode(y))));
    }
    var e = encode3(rec2020_to_srgb(c));
    if !luminance {
        e = vec3<f32>(lut(0u, e.r), lut(0u, e.g), lut(0u, e.b));
    }
    e = vec3<f32>(lut(1u, e.r), lut(2u, e.g), lut(3u, e.b));
    return srgb_to_2020(decode3(e));
}

// Steps 1–4: everything before curves.
fn before_curves(at: vec2<u32>) -> vec3<f32> {
    // The source is an *Srgb texture, so this load is already linear.
    var c = exposed(textureLoad(src, vec2<i32>(at), 0).rgb);                      // 1, 2
    c = light(c, (vec2<f32>(at) + 0.5) / vec2<f32>(textureDimensions(src)));      // 3
    return color(c);                                                              // 4
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(src);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    var c = before_curves(id.xy);
    if p.effects.y > 0.5 { c = apply_curves(c, p.effects.y > 1.5); } // 5
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
