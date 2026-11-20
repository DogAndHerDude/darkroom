// Shared by every develop pass: the parameters, colour spaces, the perceptual encoding and the
// output transform. Each pass's source is this file followed by its own.

struct Params {
    white_balance: vec4<f32>, // r, g, b multipliers
    light: vec4<f32>,         // exposure multiplier, black point, brilliance, highlights
    tone: vec4<f32>,          // shadows, brightness, contrast
    color: vec4<f32>,         // saturation, vibrance, Selective Color (1 on), black and white (1 on)
    effects: vec4<f32>,       // vignette, curves (0 off, 1 on, 2 on with the composite on luminance), probe x, probe y
    detail: vec4<f32>,        // definition (−1..1), sharpening, noise reduction (0..1), the base's window radius in quarter-size pixels
    output: vec4<f32>,        // x: 1 for Display P3 output, 0 for sRGB; y: detail scale (pixels here per preview pixel)
    // Per colour band, in band order (bands.rs), four to a vec4:
    band_hue: array<vec4<f32>, 2>,        // hue turn, radians
    band_saturation: array<vec4<f32>, 2>, // chroma scale less 1
    band_luminance: array<vec4<f32>, 2>,  // lightness exponent, base 2
    band_mix: array<vec4<f32>, 2>,        // black-and-white mixer: lightness exponent, base 2
    band_centres: array<vec4<f32>, 2>,    // Oklab hue, radians
}

const LUMA: vec3<f32> = vec3<f32>(0.2627, 0.6780, 0.0593); // Rec.2020
const LUMA_SRGB: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const EPS: f32 = 1e-6;
// How far below zero a channel may sit, as a share of the brightest, and still count as in gamut.
const GAMUT_SLACK: f32 = 1e-4;

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

// Steps 1 and 2 and the black point, from a source pixel (linear sRGB primaries): the photo as
// the tone step receives it. Every pass binds its Params as `p`.
fn exposed(srgb: vec3<f32>) -> vec3<f32> {
    let c = srgb_to_2020(srgb) * p.white_balance.rgb * p.light.x;
    // Black point: positive crushes, negative lifts, on each channel in linear light.
    let b = p.light.y * 0.03;
    return max((c - vec3<f32>(b)) / (1.0 - b), vec3<f32>(0.0));
}

// A perceptual encoding for tone work, curves and detail: a 2.2 power, extended linearly past 1
// so values brighter than white survive until the output step.
fn encode(x: f32) -> f32 {
    if x <= 1.0 { return pow(max(x, 0.0), 1.0 / 2.2); }
    return 1.0 + (x - 1.0) / 2.2;
}

fn decode(e: f32) -> f32 {
    if e <= 1.0 { return pow(max(e, 0.0), 2.2); }
    return 1.0 + (e - 1.0) * 2.2;
}

fn encode3(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(encode(c.r), encode(c.g), encode(c.b));
}

fn decode3(e: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(decode(e.r), decode(e.g), decode(e.b));
}

/// `c` scaled so its luminance becomes `y2`, keeping hue.
fn with_luma(c: vec3<f32>, y: f32, y2: f32) -> vec3<f32> {
    if y <= EPS {
        return c;
    }
    return c * (max(y2, 0.0) / y);
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

fn rec2020_to_p3(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(c, vec3<f32>(1.3435783, -0.2821797, -0.0613986)),
        dot(c, vec3<f32>(-0.0652975, 1.0757879, -0.0104905)),
        dot(c, vec3<f32>(0.0028218, -0.0195985, 1.0167767)),
    );
}

fn srgb_to_p3(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(c, vec3<f32>(0.8224621, 0.1775380, 0.0)),
        dot(c, vec3<f32>(0.0331941, 0.9668058, 0.0)),
        dot(c, vec3<f32>(0.0170827, 0.0723974, 0.9105199)),
    );
}

// ---- output ---------------------------------------------------------------------------------

// A colour in Oklab expressed in the output space: sRGB, or Display P3 when `p3`. Oklab is
// defined from linear sRGB, which holds any colour unbounded, so P3 goes through it.
fn oklab_to_output(lab: vec3<f32>, p3: bool) -> vec3<f32> {
    let c = oklab_to_srgb(lab);
    if p3 { return srgb_to_p3(c); }
    return c;
}

// The inverse of `oklab_to_output`'s last step: output-space values as linear sRGB.
fn output_to_srgb(c: vec3<f32>, p3: bool) -> vec3<f32> {
    if !p3 { return c; }
    return vec3<f32>(
        dot(c, vec3<f32>(1.2249401, -0.2249404, 0.0)),
        dot(c, vec3<f32>(-0.0420569, 1.0420571, 0.0)),
        dot(c, vec3<f32>(-0.0196376, -0.0786361, 1.0982735)),
    );
}

// `c` moved along the straight line towards the grey (r = g = b = `grey`) until no channel is
// negative. The gamut is convex in linear light, so this is one point and it moves smoothly
// with `c`.
fn towards_grey(c: vec3<f32>, grey: f32) -> vec3<f32> {
    let low = min(c.r, min(c.g, c.b));
    if low >= 0.0 {
        return c;
    }
    return mix(vec3<f32>(grey), c, grey / (grey - low));
}

// Rec.2020 working values to display-referred linear values in 0..1, in sRGB or (`p3`) Display
// P3 primaries.
fn output(c_in: vec3<f32>, p3: bool) -> vec3<f32> {
    var c = select(rec2020_to_srgb(c_in), rec2020_to_p3(c_in), p3);
    // Colours already in range are untouched, so an unedited photo comes out exactly as it went
    // in. Tiny overshoots are round-off from the colour-space trips and are just clamped. Tiny
    // is relative to the colour: a fixed allowance is several levels wide in the shadows, and
    // showed as a step where a dark ramp crossed it.
    //
    // Past white: scale all three channels down by the brightest, which keeps hue and
    // saturation (a bright yellow stays yellow rather than clipping into a band).
    let m = max(c.r, max(c.g, c.b));
    // Display P3's luminance weights; sRGB's otherwise.
    let brightness = max(dot(c, select(LUMA_SRGB, vec3<f32>(0.2290, 0.6917, 0.0793), p3)), 0.0);
    if m > 1.0 {
        c = c / m;
    }
    // Too saturated for the output space: lose chroma at constant Oklab lightness and hue.
    if min(c.r, min(c.g, c.b)) < -GAMUT_SLACK * min(m, 1.0) {
        let lab = srgb_to_oklab(output_to_srgb(c, p3));
        // The grey of the same lightness: an Oklab colour with no chroma has three equal channels.
        let grey = pow(max(lab.x, 0.0), 3.0);
        // How much chroma survives is read off a straight line in linear light, from that grey
        // to the colour, where it enters the gamut. Searching along the Oklab chroma line itself
        // doesn't work: around blue that line leaves the gamut, comes back and leaves again, so
        // the answer jumps between neighbouring colours (a hard step in a blue ramp).
        let inside = srgb_to_oklab(output_to_srgb(towards_grey(c, grey), p3));
        let k = min(length(inside.yz) / max(length(lab.yz), EPS), 1.0);
        // The Oklab colour with that chroma can still sit a little outside; the same straight
        // line brings in what is left, by too little to show as a hue shift.
        c = towards_grey(oklab_to_output(vec3<f32>(lab.x, lab.yz * k), p3), grey);
    }
    // Far past white, a colour burns out: it fades towards white with how far its luminance
    // went over, gently at first, so slightly over-bright colours keep theirs. Luminance, not
    // the brightest channel: a saturated blue has a large blue channel without being bright.
    let burn = clamp((brightness - 1.0) / 3.0, 0.0, 1.0);
    c = mix(c, vec3<f32>(1.0), burn * burn);
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}
