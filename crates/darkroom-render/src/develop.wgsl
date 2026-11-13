// Develop: the photo through every per-pixel adjustment, in the spec's fixed order:
//
//   source (sRGB) → linear Rec.2020 working space
//   1 white balance → 2 exposure → 3 light (black point, then tone) → 4 color
//   → 5 curves → 6 detail (to come: needs neighbourhood passes) → 7 vignette
//   → output: Rec.2020 → linear sRGB; past white scaled down, too-saturated colours lose chroma at
//     constant Oklab hue, far past white burns towards white
//
// The result is display-referred linear sRGB in 0..1, ready for the present pass (and later
// export and the histogram).

struct Params {
    white_balance: vec4<f32>, // r, g, b multipliers
    light: vec4<f32>,         // exposure multiplier, black point, brilliance, highlights
    tone: vec4<f32>,          // shadows, brightness, contrast
    color: vec4<f32>,         // saturation, vibrance
    effects: vec4<f32>,       // vignette, curves on
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> p: Params;
// Four curves of LUT_SIZE samples each: composite RGB, red, green, blue.
@group(0) @binding(3) var<storage, read> curves: array<f32>;

const LUT_SIZE: u32 = 1024u;
const LUMA: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593); // Rec.2020
const LUMA_SRGB: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const EPS: f32 = 1e-6;
const GAMUT_SLACK: f32 = 1e-3;

// ---- colour spaces --------------------------------------------------------------------------

fn srgb_to_2020(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(c, vec3<f32>(0.627404, 0.329283, 0.043313)),
        dot(c, vec3<f32>(0.069097, 0.919540, 0.011362)),
        dot(c, vec3<f32>(0.016391, 0.088013, 0.895595)),
    );
}

fn rec2020_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(c, vec3<f32>(1.660491, -0.587641, -0.072850)),
        dot(c, vec3<f32>(-0.124550, 1.132900, -0.008349)),
        dot(c, vec3<f32>(-0.018151, -0.100579, 1.118730)),
    );
}

// A perceptual encoding for tone work and curves: a 2.2 power, extended linearly past 1 so
// values brighter than white survive until the roll-off.
fn encode(x: f32) -> f32 {
    if x <= 1.0 { return pow(max(x, 0.0), 1.0 / 2.2); }
    return 1.0 + (x - 1.0) / 2.2;
}

fn decode(e: f32) -> f32 {
    if e <= 1.0 { return pow(max(e, 0.0), 2.2); }
    return 1.0 + (e - 1.0) * 2.2;
}

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
    var c = max((c_in - vec3<f32>(b)) / (1.0 - b), vec3<f32>(0.0));

    if all(p.tone == vec4<f32>(0.0)) && p.light.z == 0.0 && p.light.w == 0.0 {
        return c;
    }
    // Tone works on luminance and scales the channels by the ratio, so hue holds.
    let y = dot(c, LUMA);
    if y <= EPS {
        return c;
    }
    let y2 = decode(tone(encode(y)));
    return c * (y2 / y);
}

// Oklab (Björn Ottosson), from and to linear sRGB primaries. Scaling a and b changes chroma at
// constant perceived hue and lightness, which RGB mixing towards grey does not.
fn cbrt(x: f32) -> f32 {
    return sign(x) * pow(abs(x), 1.0 / 3.0);
}

fn srgb_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = cbrt(dot(c, vec3<f32>(0.4122214708, 0.5363325363, 0.0514459929)));
    let m = cbrt(dot(c, vec3<f32>(0.2119034982, 0.6806995451, 0.1073969566)));
    let s = cbrt(dot(c, vec3<f32>(0.0883024619, 0.2817188376, 0.6299787005)));
    let lms = vec3<f32>(l, m, s);
    return vec3<f32>(
        dot(lms, vec3<f32>(0.2104542553, 0.7936177850, -0.0040720468)),
        dot(lms, vec3<f32>(1.9779984951, -2.4285922050, 0.4505937099)),
        dot(lms, vec3<f32>(0.0259040371, 0.7827717662, -0.8086757660)),
    );
}

fn oklab_to_srgb(lab: vec3<f32>) -> vec3<f32> {
    let l = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let lms = vec3<f32>(l * l * l, m * m * m, s * s * s);
    return vec3<f32>(
        dot(lms, vec3<f32>(4.0767416621, -3.3077115913, 0.2309699292)),
        dot(lms, vec3<f32>(-1.2684380046, 2.6097574011, -0.3413193965)),
        dot(lms, vec3<f32>(-0.0041960863, -0.7034186147, 1.7076147010)),
    );
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
    let c = rec2020_to_srgb(c2020);
    var e = vec3<f32>(encode(c.r), encode(c.g), encode(c.b));
    e = vec3<f32>(lut(0u, e.r), lut(0u, e.g), lut(0u, e.b));
    e = vec3<f32>(lut(1u, e.r), lut(2u, e.g), lut(3u, e.b));
    return srgb_to_2020(vec3<f32>(decode(e.r), decode(e.g), decode(e.b)));
}

// ---- 7 vignette -----------------------------------------------------------------------------

fn vignette(c: vec3<f32>, uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let amount = p.effects.x;
    if amount <= 0.0 {
        return c;
    }
    let d = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let r = length(d) / length(vec2<f32>(aspect, 1.0) * 0.5);
    let fall = smoothstep(0.3, 1.0, r);
    return c * exp2(-2.0 * amount * fall * fall);
}

// ---- output ---------------------------------------------------------------------------------

fn output(c_in: vec3<f32>) -> vec3<f32> {
    var c = rec2020_to_srgb(c_in);
    // Colours already in range are untouched, so an unedited photo comes out exactly as it went
    // in. Tiny overshoots are round-off from the colour-space trips and are just clamped.
    //
    // Past white: scale all three channels down by the brightest, which keeps hue and
    // saturation (a bright yellow stays yellow rather than clipping into a band).
    let m = max(c.r, max(c.g, c.b));
    let brightness = max(dot(c, LUMA_SRGB), 0.0);
    if m > 1.0 {
        c = c / m;
    }
    // Too saturated for sRGB: lose chroma at constant Oklab lightness and hue until no channel
    // is negative (a bisection on the chroma scale).
    if min(c.r, min(c.g, c.b)) < -GAMUT_SLACK {
        let lab = srgb_to_oklab(c);
        var lo = 0.0;
        var hi = 1.0;
        for (var i = 0; i < 14; i++) {
            let k = (lo + hi) * 0.5;
            let t = oklab_to_srgb(vec3<f32>(lab.x, lab.yz * k));
            if min(t.r, min(t.g, t.b)) < -GAMUT_SLACK { hi = k; } else { lo = k; }
        }
        c = oklab_to_srgb(vec3<f32>(lab.x, lab.yz * lo));
    }
    // Far past white, a colour burns out: it fades towards white with how far its luminance
    // went over, gently at first, so slightly over-bright colours keep theirs. Luminance, not
    // the brightest channel: a saturated blue has a large blue channel without being bright.
    let burn = clamp((brightness - 1.0) / 3.0, 0.0, 1.0);
    c = mix(c, vec3<f32>(1.0), burn * burn);
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
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
                                                // 6 detail: separate passes, to come
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size);
    c = vignette(c, uv, f32(size.x) / f32(size.y)); // 7

    textureStore(dst, vec2<i32>(id.xy), vec4<f32>(output(c), 1.0));
}

// The curve point pickers' probe: one pixel's value as the curves receive it (display red, green
// and blue in the curves' encoding), at the pixel `p.effects.zw`.
@group(0) @binding(4) var<storage, read_write> probed: array<f32, 4>;

@compute @workgroup_size(1)
fn probe() {
    let size = textureDimensions(src);
    let at = min(vec2<u32>(p.effects.zw), size - vec2<u32>(1u));
    let c = rec2020_to_srgb(before_curves(at));
    probed[0] = encode(c.r);
    probed[1] = encode(c.g);
    probed[2] = encode(c.b);
    probed[3] = 1.0;
}
