//! Rendering a developed photo at output size, for writing to a file.
//!
//! After [`Develop::update`] has developed a (full-resolution) photo, [`Develop::export`] samples
//! it through the edit's geometry into an output-sized float texture and reads that back. A 40 MP
//! float image is about 320 MB, more than a GPU buffer may hold, so it comes back in strips.

use std::sync::{Arc, Mutex};

use crate::Develop;
use crate::readback::Readback;

/// A strip's most bytes: well under every GPU's buffer limit.
const STRIP_BYTES: u64 = 64 << 20;
/// Rgba16Float.
const BYTES_PER_PIXEL: u32 = 8;

/// An exported image: display-referred linear RGBA as half floats (`f16` bits), 4 per pixel,
/// row by row.
pub struct Exported {
    pub size: [u32; 2],
    pub rgba16f: Vec<u16>,
}

impl Develop {
    /// Renders the developed photo at `out` pixels through `map` (output → source, as
    /// `Geometry::output_to_source`) and calls `done` with it from another thread. Call after
    /// [`Develop::update`] has developed the edit.
    pub fn export(&self, device: &wgpu::Device, queue: &wgpu::Queue, map: [f32; 6], out: [u32; 2], done: impl FnOnce(Result<Exported, String>) + Send + 'static) {
        let Some(photo) = &self.photo else {
            done(Err("no photo to export".into()));
            return;
        };
        let max = device.limits().max_texture_dimension_2d;
        if out[0] > max || out[1] > max {
            done(Err(format!("{} × {} px is larger than the GPU can render ({max} px)", out[0], out[1])));
            return;
        }

        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("export"),
            entries: &[
                entry(0, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba16Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                }),
                entry(3, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("export"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("export"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 })),
            module: &device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("export"), source: wgpu::ShaderSource::Wgsl(include_str!("export.wgsl").into()) }),
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("export"),
            size: wgpu::Extent3d { width: out[0], height: out[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("export map"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let [a, b, c, d, e, f] = map;
        queue.write_buffer(&uniform, 0, bytemuck::cast_slice(&[a, b, c, d, e, f, 0.0, 0.0]));
        let target_view = target.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("export"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&photo.working) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&target_view) },
                wgpu::BindGroupEntry { binding: 3, resource: uniform.as_entire_binding() },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("export") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("export"), timestamp_writes: None });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(out[0].div_ceil(8), out[1].div_ceil(8), 1);
        }
        // Strips of whole rows, each row padded to 256 bytes as copies require.
        let row_bytes = out[0] * BYTES_PER_PIXEL;
        let padded = row_bytes.div_ceil(256) * 256;
        let budget = STRIP_BYTES.min(device.limits().max_buffer_size);
        let rows_per_strip = ((budget / padded as u64) as u32).clamp(1, out[1]);
        let strips: Vec<(u32, u32, Readback)> = (0..out[1])
            .step_by(rows_per_strip as usize)
            .map(|y| {
                let rows = rows_per_strip.min(out[1] - y);
                (y, rows, Readback::new(device, "export strip", padded as u64 * rows as u64))
            })
            .collect();
        for (y, rows, readback) in &strips {
            readback.copy_rows(&mut encoder, &target, *y, *rows, padded);
        }
        queue.submit([encoder.finish()]);

        // Put the strips back together as they arrive; the last one in finishes the image.
        let count = strips.len();
        let parts: Arc<Mutex<Vec<Option<Vec<u8>>>>> = Arc::new(Mutex::new(vec![None; count]));
        let done: Arc<Mutex<Option<Box<dyn FnOnce(Result<Exported, String>) + Send>>>> = Arc::new(Mutex::new(Some(Box::new(done))));
        for (i, (_, rows, readback)) in strips.into_iter().enumerate() {
            let (parts, done) = (parts.clone(), done.clone());
            readback.start(device, move |bytes| {
                let finish = |result| {
                    if let Some(f) = done.lock().ok().and_then(|mut d| d.take()) {
                        f(result);
                    }
                };
                let Some(bytes) = bytes else {
                    finish(Err("the GPU didn't finish rendering the export".into()));
                    return;
                };
                let mut parts = parts.lock().unwrap_or_else(|e| e.into_inner());
                // Drop each row's padding as the strip arrives.
                let mut packed = Vec::with_capacity((row_bytes * rows) as usize);
                for r in 0..rows as usize {
                    packed.extend_from_slice(&bytes[r * padded as usize..r * padded as usize + row_bytes as usize]);
                }
                parts[i] = Some(packed);
                if parts.iter().all(Option::is_some) {
                    let mut rgba16f = Vec::with_capacity((out[0] * out[1] * 4) as usize);
                    for part in parts.iter_mut() {
                        rgba16f.extend(part.take().unwrap_or_default().chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])));
                    }
                    drop(parts);
                    finish(Ok(Exported { size: out, rgba16f }));
                }
            });
        }
    }
}
