// Develop, last pass: steps 6 and 7 and the output transform, from the (denoised) stage into
// the working texture the present pass and the histogram read.
//
//   6 detail: sharpening, then definition → 7 vignette → output
//
// Both detail adjustments change luminance only (channels scaled by the ratio, so hue holds),
// in the perceptual encoding.

@group(0) @binding(0) var stage: texture_2d<f32>;
// The stage's edge-aware base at quarter size, as a and b (base.wgsl).
@group(0) @binding(1) var base: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
@group(0) @binding(3) var working: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var<uniform> p: Params;

// Sharpening ignores differences smaller than this (encoded units, about 2.5 levels of 255), so
// it brings out edges more than noise.
const SHARPEN_THRESHOLD: f32 = 0.01;

// Encoded luminance at a position in pixels, between pixels by bilinear interpolation.
fn luma_at(pos: vec2<f32>, size: vec2<i32>) -> f32 {
    return encode(dot(textureSampleLevel(stage, smp, pos / vec2<f32>(size), 0.0).rgb, LUMA));
}

// A 5 × 5 binomial blur of encoded luminance, the unsharp mask's reference: about σ = 1 preview
// pixel. The taps sit `step` pixels apart, so a render at another size blurs the same detail.
fn fine_blur(at: vec2<i32>, size: vec2<i32>, step: f32) -> f32 {
    let centre = vec2<f32>(at) + 0.5;
    var k = array<f32, 5>(1.0, 4.0, 6.0, 4.0, 1.0);
    var sum = 0.0;
    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            sum += luma_at(centre + vec2<f32>(f32(dx), f32(dy)) * step, size) * k[dx + 2] * k[dy + 2];
        }
    }
    return sum / 256.0;
}

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

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(stage));
    let at = vec2<i32>(id.xy);
    if at.x >= size.x || at.y >= size.y {
        return;
    }
    var c = textureLoad(stage, at, 0).rgb;
    let uv = (vec2<f32>(at) + 0.5) / vec2<f32>(size);

    let definition = p.detail.x;
    let sharpening = p.detail.y;
    if definition != 0.0 || sharpening > 0.0 {
        let y = dot(c, LUMA);
        let e0 = encode(y);
        let fine = fine_blur(at, size, max(p.output.y, 0.25));
        var e = e0;
        if sharpening > 0.0 {
            let d = e0 - fine;
            e += sharpening * 1.5 * sign(d) * max(abs(d) - SHARPEN_THRESHOLD, 0.0);
        }
        if definition != 0.0 {
            // Local contrast: the lightly blurred value against its region's base, a band that
            // leaves pixel-level noise out (that's sharpening's job), strongest in the midtones
            // so shadows and highlights don't clip. Negative values soften. Beside an edge the
            // base follows the pixel, so the difference fades there and leaves no halo.
            let ab = textureSampleLevel(base, smp, uv, 0.0).rg;
            let d = fine - (ab.x * fine + ab.y);
            let q = clamp(e0, 0.0, 1.0);
            e += definition * 0.8 * d * clamp(4.0 * q * (1.0 - q), 0.0, 1.0);
        }
        c = with_luma(c, y, decode(e));
    }

    c = vignette(c, uv, f32(size.x) / f32(size.y));
    textureStore(working, at, vec4<f32>(output(c, p.output.x > 0.5), 1.0));
}
