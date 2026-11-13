// Export: the developed photo through its geometry (crop, straighten, rotate, flip) into an
// output-sized texture, sampled bilinearly. Values stay display-referred and linear; the file
// encoder applies the transfer curve.

struct Map {
    // Output (0..1) → source (0..1): x' = a·x + b·y + e, y' = c·x + d·y + f.
    abcd: vec4<f32>,
    ef: vec4<f32>,
}

@group(0) @binding(0) var working: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var<uniform> m: Map;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(out);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size);
    let src = vec2<f32>(m.abcd.x * uv.x + m.abcd.y * uv.y + m.ef.x, m.abcd.z * uv.x + m.abcd.w * uv.y + m.ef.y);
    let c = textureSampleLevel(working, smp, clamp(src, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    textureStore(out, vec2<i32>(id.xy), vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0));
}
