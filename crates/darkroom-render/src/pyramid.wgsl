// The wide blur Definition compares against: the stage's encoded luminance at quarter size,
// blurred with a Gaussian in two one-dimensional passes. A quarter of the pixels and a separable
// kernel keep a wide radius cheap; finish.wgsl samples it back up with bilinear filtering.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> p: Params;

const MAX_RADIUS: i32 = 32;

// Full-size stage → quarter-size encoded luminance: the mean of each 4 × 4 block.
@compute @workgroup_size(8, 8)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let out_size = vec2<i32>(textureDimensions(dst));
    let at = vec2<i32>(id.xy);
    if at.x >= out_size.x || at.y >= out_size.y {
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
    textureStore(dst, at, vec4<f32>(sum / 16.0, 0.0, 0.0, 1.0));
}

fn blur(at: vec2<i32>, step: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(src));
    let sigma = max(p.detail.w, 0.5);
    let radius = min(i32(ceil(3.0 * sigma)), MAX_RADIUS);
    var sum = 0.0;
    var weight = 0.0;
    for (var i = -radius; i <= radius; i++) {
        let q = clamp(at + step * i, vec2<i32>(0), size - vec2<i32>(1));
        let w = exp(-f32(i * i) / (2.0 * sigma * sigma));
        sum += textureLoad(src, q, 0).r * w;
        weight += w;
    }
    return sum / weight;
}

@compute @workgroup_size(8, 8)
fn blur_h(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = vec2<i32>(id.xy);
    if any(at >= vec2<i32>(textureDimensions(dst))) {
        return;
    }
    textureStore(dst, at, vec4<f32>(blur(at, vec2<i32>(1, 0)), 0.0, 0.0, 1.0));
}

@compute @workgroup_size(8, 8)
fn blur_v(@builtin(global_invocation_id) id: vec3<u32>) {
    let at = vec2<i32>(id.xy);
    if any(at >= vec2<i32>(textureDimensions(dst))) {
        return;
    }
    textureStore(dst, at, vec4<f32>(blur(at, vec2<i32>(0, 1)), 0.0, 0.0, 1.0));
}
