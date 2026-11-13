// Noise reduction: an edge-preserving (bilateral) blur of the stage. Neighbours count by how close
// they are and by how similar their colour is, so grain and JPEG noise smooth out while edges,
// which differ by more than the noise does, stay put. Stronger settings widen both.
//
// The reach is set in preview pixels. A render at another size (export) scales it: the same
// 9 × 9 taps, spread `stride` pixels apart, so the cost per pixel doesn't grow with the size.

@group(0) @binding(0) var stage: texture_2d<f32>;
@group(0) @binding(1) var smoothed: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> p: Params;

const RADIUS: i32 = 4;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(stage));
    let at = vec2<i32>(id.xy);
    if at.x >= size.x || at.y >= size.y {
        return;
    }
    let strength = p.detail.z;
    // In the perceptual encoding, so a difference means about the same in shadows and highlights.
    let centre = encode3(textureLoad(stage, at, 0).rgb);
    // At full strength, differences up to about 0.15 (typical noise between neighbours) still
    // average out, while an edge of 0.5 keeps well under 1% weight.
    let sigma_range = 0.02 + 0.13 * strength;
    let scale = max(p.output.y, 0.25);
    let sigma_space = (1.0 + 2.0 * strength) * scale;
    let stride = max(i32(ceil(scale - 0.01)), 1);

    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var dy = -RADIUS; dy <= RADIUS; dy++) {
        for (var dx = -RADIUS; dx <= RADIUS; dx++) {
            let o = vec2<i32>(dx, dy) * stride;
            let q = clamp(at + o, vec2<i32>(0), size - vec2<i32>(1));
            let e = encode3(textureLoad(stage, q, 0).rgb);
            let d = e - centre;
            let w = exp(-f32(dot(o, o)) / (2.0 * sigma_space * sigma_space)) * exp(-dot(d, d) / (2.0 * sigma_range * sigma_range));
            sum += e * w;
            weight += w;
        }
    }
    textureStore(smoothed, at, vec4<f32>(decode3(sum / weight), 1.0));
}
