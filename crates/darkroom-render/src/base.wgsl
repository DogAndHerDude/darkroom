// The edge-aware base: a photo's luminance with texture smoothed away and edges kept, at quarter
// size. Local tone lights each region by its base, and Definition measures detail against it;
// because the base doesn't blur across an edge, neither leaves a halo beside one.
//
// It is a guided filter (He, Sun and Tang, 2010) with the luminance guiding itself: around each
// pixel the base is modelled as a·luminance + b. Where a window holds an edge (its variance is
// large against EPSILON) a is near 1 and the base follows the pixel; where it holds only texture
// a is near 0 and the base is the window's mean. The passes leave a and b, averaged over the
// window, and the full-size passes rebuild the base from their own luminance, so it is sharp
// at full size although it was computed at a quarter.
//
//   downsample → box_h → coefficients → box_h → box_v
//
// A quarter of the pixels and separable box means keep a wide window cheap.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> p: Params;

const MAX_RADIUS: i32 = 40;
// The variance (in encoded units squared) that counts as an edge: a step of about 0.2 between
// two regions. A guess, set by eye on one synthetic backlit scene.
const EPSILON: f32 = 0.01;

// The mean of each 4 × 4 block's encoded luminance, and its square for the variance.
fn store_block(at: vec2<i32>, sum: f32) {
    let mean = sum / 16.0;
    textureStore(dst, at, vec4<f32>(mean, mean * mean, 0.0, 1.0));
}

// The photo as the tone step receives it → quarter size, for local tone.
@compute @workgroup_size(8, 8)
fn downsample_source(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = vec2<i32>(id.xy);
    if any(at >= vec2<i32>(textureDimensions(dst))) {
        return;
    }
    let size = vec2<i32>(textureDimensions(src));
    var sum = 0.0;
    for (var dy = 0; dy < 4; dy++) {
        for (var dx = 0; dx < 4; dx++) {
            let q = min(at * 4 + vec2<i32>(dx, dy), size - vec2<i32>(1));
            sum += encode(dot(exposed(textureLoad(src, q, 0).rgb), LUMA));
        }
    }
    store_block(at, sum);
}

// The stage → quarter size, for Definition.
@compute @workgroup_size(8, 8)
fn downsample_stage(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = vec2<i32>(id.xy);
    if any(at >= vec2<i32>(textureDimensions(dst))) {
        return;
    }
    let size = vec2<i32>(textureDimensions(src));
    var sum = 0.0;
    for (var dy = 0; dy < 4; dy++) {
        for (var dx = 0; dx < 4; dx++) {
            let q = min(at * 4 + vec2<i32>(dx, dy), size - vec2<i32>(1));
            sum += encode(dot(textureLoad(src, q, 0).rgb, LUMA));
        }
    }
    store_block(at, sum);
}

// The mean of the first two channels along a line of the window's width.
fn box(at: vec2<i32>, step: vec2<i32>) -> vec2<f32> {
    let size = vec2<i32>(textureDimensions(src));
    let radius = clamp(i32(p.detail.w), 1, MAX_RADIUS);
    var sum = vec2<f32>(0.0);
    for (var i = -radius; i <= radius; i++) {
        sum += textureLoad(src, clamp(at + step * i, vec2<i32>(0), size - vec2<i32>(1)), 0).rg;
    }
    return sum / f32(2 * radius + 1);
}

@compute @workgroup_size(8, 8)
fn box_h(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = vec2<i32>(id.xy);
    if any(at >= vec2<i32>(textureDimensions(dst))) {
        return;
    }
    textureStore(dst, at, vec4<f32>(box(at, vec2<i32>(1, 0)), 0.0, 1.0));
}

@compute @workgroup_size(8, 8)
fn box_v(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = vec2<i32>(id.xy);
    if any(at >= vec2<i32>(textureDimensions(dst))) {
        return;
    }
    textureStore(dst, at, vec4<f32>(box(at, vec2<i32>(0, 1)), 0.0, 1.0));
}

// The second direction of the window's mean and mean square, turned into its a and b.
@compute @workgroup_size(8, 8)
fn coefficients(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = vec2<i32>(id.xy);
    if any(at >= vec2<i32>(textureDimensions(dst))) {
        return;
    }
    let m = box(at, vec2<i32>(0, 1));
    let variance = max(m.y - m.x * m.x, 0.0);
    let a = variance / (variance + EPSILON);
    textureStore(dst, at, vec4<f32>(a, m.x - a * m.x, 0.0, 1.0));
}
