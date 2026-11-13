// Shared by every develop pass: the parameters, colour spaces, the perceptual encoding and the
// output transform. Each pass's source is this file followed by its own.

struct Params {
    white_balance: vec4<f32>, // r, g, b multipliers
    light: vec4<f32>,         // exposure multiplier, black point, brilliance, highlights
    tone: vec4<f32>,          // shadows, brightness, contrast
    color: vec4<f32>,         // saturation, vibrance
    effects: vec4<f32>,       // vignette, curves on, probe x, probe y
    detail: vec4<f32>,        // definition (−1..1), sharpening, noise reduction (0..1), definition blur σ in quarter-size pixels
}

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

// ---- output ---------------------------------------------------------------------------------

// Rec.2020 working values to display-referred linear sRGB in 0..1.
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
