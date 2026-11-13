// Histogram of the developed photo: 256 bins each of luminance, red, green and blue, counted on
// sRGB-encoded values, so it matches what's on screen and the curves' axes.
//
// Counts go straight into the global bins. A per-workgroup stage in `var<workgroup>` memory
// would cut contention, but RUNE's Metal backend returns zeros for workgroup atomics, and at
// preview size the direct version is fast enough.

@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>, 1024>;

fn srgb_encode(c: f32) -> f32 {
    if c <= 0.0031308 { return c * 12.92; }
    return 1.055 * pow(c, 1.0 / 2.4) - 0.055;
}

fn bin(v: f32) -> u32 {
    return u32(clamp(v, 0.0, 1.0) * 255.0 + 0.5);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(photo);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let c = clamp(textureLoad(photo, vec2<i32>(id.xy), 0).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let y = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    atomicAdd(&bins[bin(srgb_encode(y))], 1u);
    atomicAdd(&bins[256u + bin(srgb_encode(c.r))], 1u);
    atomicAdd(&bins[512u + bin(srgb_encode(c.g))], 1u);
    atomicAdd(&bins[768u + bin(srgb_encode(c.b))], 1u);
}
