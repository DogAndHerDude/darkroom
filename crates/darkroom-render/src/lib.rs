//! Darkroom's GPU develop pipeline.
//!
//! [`Develop`] owns a photo on the GPU: the source texture (sRGB 8-bit, sampled as linear), a
//! float working texture the develop compute pass writes, and the pipelines that draw the result
//! into a render pass. It never creates a device: it runs on whichever one it is handed, which in
//! the app is RUNE's own, so the preview never leaves the GPU.
//!
//! What the UI needs back from the GPU, the histogram of the edit and the curve pickers' probe,
//! arrives in [`Results`] from a background thread, which then calls the wake function so the
//! UI picks it up.

mod export;
mod params;
mod readback;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use darkroom_core::{Curves, Edit};

pub use export::Exported;
pub use params::Params;
use readback::Readback;

/// Samples per curve in the lookup table the develop shader reads (`LUT_SIZE` there).
const LUT_SIZE: usize = 1024;
const HISTOGRAM_BYTES: u64 = 4 * 256 * 4;
const PROBE_BYTES: u64 = 4 * 4;

/// 256 bins each of luminance, red, green and blue, on sRGB-encoded values.
pub type Histogram = [[u32; 256]; 4];

/// Called from a background thread when new results are in.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Results from the GPU, shared with the UI.
#[derive(Default)]
pub struct Results {
    histogram: Mutex<Option<Histogram>>,
    probe: Mutex<Option<Probed>>,
}

/// One pixel as the curves receive it: display red, green and blue in the curves' 0..1 encoding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probed {
    /// The request this answers (see [`Develop::probe`]).
    pub id: u64,
    pub rgb: [f32; 3],
}

impl Results {
    /// The latest histogram of the edited photo, once.
    pub fn take_histogram(&self) -> Option<Histogram> {
        self.histogram.lock().ok()?.take()
    }

    /// The latest probe result, once.
    pub fn take_probe(&self) -> Option<Probed> {
        self.probe.lock().ok()?.take()
    }
}

/// Where to draw the photo inside a render pass.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// The box to fit the photo into, in the pass's pixels: x, y, width, height.
    pub rect: [f32; 4],
    /// The pass target's size in pixels.
    pub target: [u32; 2],
    pub format: wgpu::TextureFormat,
    /// Encode to sRGB in the shader (the frame blends sRGB-encoded values in a non-sRGB target).
    pub encode_srgb: bool,
    /// What's shown: its size (for its proportions) and the map from it to the photo
    /// (`Geometry::output_to_source`, or the uncropped canvas while cropping).
    pub output: [f32; 2],
    pub map: [f32; 6],
    /// Linear RGB shown off the photo, where a straightened photo's corners turn away.
    pub background: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PresentUniform {
    ndc: [f32; 4],
    encode: [f32; 4],
    map_abcd: [f32; 4],
    map_ef: [f32; 4],
    background: [f32; 4],
}

const WORKING_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The colour maths every develop pass shares; each pass's source is this plus its own.
const COMMON: &str = include_str!("common.wgsl");

struct Photo {
    id: u64,
    size: [u32; 2],
    /// The definition blur's size: a quarter of the photo's, rounded up.
    quarter: [u32; 2],
    /// Source → stage (steps 1–5).
    develop_group: wgpu::BindGroup,
    /// Stage → denoised stage.
    denoise_group: wgpu::BindGroup,
    /// Stage (or denoised stage) → quarter A, then A → B → A for the two blur directions.
    downsample_group: wgpu::BindGroup,
    downsample_denoised_group: wgpu::BindGroup,
    blur_h_group: wgpu::BindGroup,
    blur_v_group: wgpu::BindGroup,
    /// Stage (or denoised stage) and the blur → working texture.
    finish_group: wgpu::BindGroup,
    finish_denoised_group: wgpu::BindGroup,
    histogram_group: wgpu::BindGroup,
    present_group: wgpu::BindGroup,
    /// The developed result, for export.
    working: wgpu::TextureView,
}

pub struct Develop {
    develop_layout: wgpu::BindGroupLayout,
    develop_pipeline: wgpu::ComputePipeline,
    probe_pipeline: wgpu::ComputePipeline,
    denoise_layout: wgpu::BindGroupLayout,
    denoise_pipeline: wgpu::ComputePipeline,
    pyramid_layout: wgpu::BindGroupLayout,
    downsample_pipeline: wgpu::ComputePipeline,
    blur_h_pipeline: wgpu::ComputePipeline,
    blur_v_pipeline: wgpu::ComputePipeline,
    finish_layout: wgpu::BindGroupLayout,
    finish_pipeline: wgpu::ComputePipeline,
    histogram_layout: wgpu::BindGroupLayout,
    histogram_pipeline: wgpu::ComputePipeline,
    params: wgpu::Buffer,
    curves: wgpu::Buffer,
    bins: wgpu::Buffer,
    probed: wgpu::Buffer,
    histogram_readback: Readback,
    probe_readback: Readback,
    present_layout: wgpu::BindGroupLayout,
    present_pipeline_layout: wgpu::PipelineLayout,
    present_module: wgpu::ShaderModule,
    present_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    present_uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    photo: Option<Photo>,
    /// The edit the working texture holds, to skip the pass when nothing changed.
    developed: Option<Edit>,
    /// The curves in the lookup table.
    uploaded_curves: Option<Curves>,
    /// The working texture changed since the last histogram was read.
    histogram_stale: bool,
    /// A probe asked for while the last one was still being read: (id, pixel).
    pending_probe: Option<(u64, [u32; 2])>,
    results: Arc<Results>,
    wake: Wake,
    /// Develop for Display P3 instead of sRGB (export only; the preview is sRGB).
    pub display_p3: bool,
    /// Count the histogram after each develop (off for export).
    pub histogram_enabled: bool,
}

impl Develop {
    pub fn new(device: &wgpu::Device, results: Arc<Results>, wake: Wake) -> Self {
        let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None };
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None };
        let storage = |read_only| wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None };
        let texture = |filterable| wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let compute = |label, layout: &wgpu::BindGroupLayout, source: &str, entry_point| {
            let source = format!("{COMMON}\n{source}");
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(label),
                    bind_group_layouts: &[Some(layout)],
                    immediate_size: 0,
                })),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };

        let cs = wgpu::ShaderStages::COMPUTE;
        let write = wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: WORKING_FORMAT,
            view_dimension: wgpu::TextureViewDimension::D2,
        };
        let develop_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("develop"),
            entries: &[entry(0, cs, texture(false)), entry(1, cs, write), entry(2, cs, uniform), entry(3, cs, storage(true)), entry(4, cs, storage(false))],
        });
        let develop_pipeline = compute("develop", &develop_layout, include_str!("develop.wgsl"), "main");
        let probe_pipeline = compute("probe", &develop_layout, include_str!("develop.wgsl"), "probe");

        // Noise reduction and the definition blur share a shape: read one texture, write another.
        let read_write_layout = |label| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[entry(0, cs, texture(false)), entry(1, cs, write), entry(2, cs, uniform)],
            })
        };
        let denoise_layout = read_write_layout("denoise");
        let denoise_pipeline = compute("denoise", &denoise_layout, include_str!("denoise.wgsl"), "main");
        let pyramid_layout = read_write_layout("pyramid");
        let downsample_pipeline = compute("downsample", &pyramid_layout, include_str!("pyramid.wgsl"), "downsample");
        let blur_h_pipeline = compute("blur h", &pyramid_layout, include_str!("pyramid.wgsl"), "blur_h");
        let blur_v_pipeline = compute("blur v", &pyramid_layout, include_str!("pyramid.wgsl"), "blur_v");

        let finish_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("finish"),
            entries: &[
                entry(0, cs, texture(false)),
                entry(1, cs, texture(true)),
                entry(2, cs, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(3, cs, write),
                entry(4, cs, uniform),
            ],
        });
        let finish_pipeline = compute("finish", &finish_layout, include_str!("finish.wgsl"), "main");

        let histogram_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("histogram"),
            entries: &[entry(0, cs, texture(false)), entry(1, cs, storage(false))],
        });
        let histogram_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("histogram"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("histogram"),
                bind_group_layouts: &[Some(&histogram_layout)],
                immediate_size: 0,
            })),
            module: &device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("histogram"),
                source: wgpu::ShaderSource::Wgsl(include_str!("histogram.wgsl").into()),
            }),
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let present_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                entry(0, wgpu::ShaderStages::FRAGMENT, texture(true)),
                entry(1, wgpu::ShaderStages::FRAGMENT, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, vf, uniform),
            ],
        });
        let present_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("present"),
            bind_group_layouts: &[Some(&present_layout)],
            immediate_size: 0,
        });
        let present_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("present"),
            source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });

        let buffer = |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
        let uniform_usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let storage_usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("present"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Develop {
            develop_layout,
            develop_pipeline,
            probe_pipeline,
            denoise_layout,
            denoise_pipeline,
            pyramid_layout,
            downsample_pipeline,
            blur_h_pipeline,
            blur_v_pipeline,
            finish_layout,
            finish_pipeline,
            histogram_layout,
            histogram_pipeline,
            params: buffer("develop params", size_of::<Params>() as u64, uniform_usage),
            curves: buffer("curves", (4 * LUT_SIZE * size_of::<f32>()) as u64, storage_usage),
            bins: buffer("histogram bins", HISTOGRAM_BYTES, storage_usage),
            probed: buffer("probe", PROBE_BYTES, storage_usage),
            histogram_readback: Readback::new(device, "histogram readback", HISTOGRAM_BYTES),
            probe_readback: Readback::new(device, "probe readback", PROBE_BYTES),
            present_layout,
            present_pipeline_layout,
            present_module,
            present_pipelines: HashMap::new(),
            present_uniform: buffer("present", size_of::<PresentUniform>() as u64, uniform_usage),
            sampler,
            photo: None,
            developed: None,
            uploaded_curves: None,
            histogram_stale: false,
            pending_probe: None,
            results,
            wake,
            display_p3: false,
            histogram_enabled: true,
        }
    }

    /// The id of the photo on the GPU, if any.
    pub fn photo_id(&self) -> Option<u64> {
        self.photo.as_ref().map(|p| p.id)
    }

    /// Uploads a photo (tightly packed sRGB RGBA8) and makes its working texture.
    pub fn set_photo(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, id: u64, size: [u32; 2], rgba: &[u8]) {
        assert_eq!(rgba.len(), (size[0] * size[1] * 4) as usize, "rgba is width × height × 4 bytes");
        let quarter = [size[0].div_ceil(4), size[1].div_ceil(4)];
        let extent = |[width, height]: [u32; 2]| wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = |label, at: [u32; 2], format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent(at),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let source = texture("photo source", size, wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        queue.write_texture(
            source.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * 4), rows_per_image: Some(size[1]) },
            extent(size),
        );
        let float = |label, at| texture(label, at, WORKING_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING).create_view(&Default::default());
        let source_view = source.create_view(&Default::default());
        let stage_view = float("photo stage", size);
        let denoised_view = float("photo denoised", size);
        let quarter_a = float("definition blur a", quarter);
        let quarter_b = float("definition blur b", quarter);
        let working_view = float("photo working", size);

        let view = |v| wgpu::BindingResource::TextureView(v);
        let group = |label, layout, entries: &[wgpu::BindingResource<'_>]| {
            let entries: Vec<_> = entries.iter().enumerate().map(|(i, r)| wgpu::BindGroupEntry { binding: i as u32, resource: r.clone() }).collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries })
        };
        let params = self.params.as_entire_binding();
        let develop_group = group(
            "develop",
            &self.develop_layout,
            &[view(&source_view), view(&stage_view), params.clone(), self.curves.as_entire_binding(), self.probed.as_entire_binding()],
        );
        let denoise_group = group("denoise", &self.denoise_layout, &[view(&stage_view), view(&denoised_view), params.clone()]);
        let downsample_group = group("downsample", &self.pyramid_layout, &[view(&stage_view), view(&quarter_a), params.clone()]);
        let downsample_denoised_group = group("downsample", &self.pyramid_layout, &[view(&denoised_view), view(&quarter_a), params.clone()]);
        let blur_h_group = group("blur h", &self.pyramid_layout, &[view(&quarter_a), view(&quarter_b), params.clone()]);
        let blur_v_group = group("blur v", &self.pyramid_layout, &[view(&quarter_b), view(&quarter_a), params.clone()]);
        let sampler = wgpu::BindingResource::Sampler(&self.sampler);
        let finish_group = group("finish", &self.finish_layout, &[view(&stage_view), view(&quarter_a), sampler.clone(), view(&working_view), params.clone()]);
        let finish_denoised_group = group("finish", &self.finish_layout, &[view(&denoised_view), view(&quarter_a), sampler, view(&working_view), params]);
        let histogram_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("histogram"),
            layout: &self.histogram_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&working_view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.bins.as_entire_binding() },
            ],
        });
        let present_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &self.present_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&working_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.present_uniform.as_entire_binding() },
            ],
        });
        self.photo = Some(Photo {
            id,
            size,
            quarter,
            develop_group,
            denoise_group,
            downsample_group,
            downsample_denoised_group,
            blur_h_group,
            blur_v_group,
            finish_group,
            finish_denoised_group,
            histogram_group,
            present_group,
            working: working_view,
        });
        self.developed = None;
    }

    /// Asks for one pixel's value before curves (photo pixel coordinates). The answer arrives
    /// in [`Results::take_probe`] tagged with `id`.
    pub fn probe(&mut self, id: u64, pixel: [u32; 2]) {
        self.pending_probe = Some((id, pixel));
    }

    /// Brings the GPU up to date with `edit`: develops when the photo or edit changed, then
    /// counts the histogram and runs a pending probe, each when its readback is free (a busy
    /// one is retried on a later call). Submits its own work so readbacks can start at once;
    /// draws later in the frame see the result, as the queue runs in order.
    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, edit: &Edit) {
        let Some(photo) = &self.photo else { return };
        let develop = self.developed.as_ref() != Some(edit);
        let probe = self.pending_probe.filter(|_| self.probe_readback.is_idle());
        self.histogram_stale |= develop && self.histogram_enabled;
        let histogram = self.histogram_stale && self.histogram_readback.is_idle();
        if !develop && probe.is_none() && !histogram {
            return;
        }

        if self.uploaded_curves.as_ref() != Some(&edit.curves) {
            let lut: Vec<f32> = [&edit.curves.rgb, &edit.curves.red, &edit.curves.green, &edit.curves.blue].iter().flat_map(|c| c.lut(LUT_SIZE)).collect();
            queue.write_buffer(&self.curves, 0, bytemuck::cast_slice(&lut));
            self.uploaded_curves = Some(edit.curves.clone());
        }
        let mut params = Params::from_edit(edit);
        if let Some((_, [x, y])) = probe {
            params.effects[2] = x as f32;
            params.effects[3] = y as f32;
        }
        params.detail[3] = definition_sigma(photo.quarter);
        params.output[0] = if self.display_p3 { 1.0 } else { 0.0 };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        let [definition, _, noise, _] = params.detail;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("develop") });
        // One compute pass per step, so each reads what the one before it finished writing.
        let mut run = |label, pipeline: &wgpu::ComputePipeline, group: &wgpu::BindGroup, [w, h]: [u32; 2], per_group: u32| {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(label), timestamp_writes: None });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(w.div_ceil(per_group), h.div_ceil(per_group), 1);
        };
        if develop {
            run("develop", &self.develop_pipeline, &photo.develop_group, photo.size, 8);
            let denoised = noise > 0.0;
            if denoised {
                run("denoise", &self.denoise_pipeline, &photo.denoise_group, photo.size, 8);
            }
            if definition != 0.0 {
                let down = if denoised { &photo.downsample_denoised_group } else { &photo.downsample_group };
                run("downsample", &self.downsample_pipeline, down, photo.quarter, 8);
                run("blur h", &self.blur_h_pipeline, &photo.blur_h_group, photo.quarter, 8);
                run("blur v", &self.blur_v_pipeline, &photo.blur_v_group, photo.quarter, 8);
            }
            let finish = if denoised { &photo.finish_denoised_group } else { &photo.finish_group };
            run("finish", &self.finish_pipeline, finish, photo.size, 8);
        }
        if probe.is_some() {
            run("probe", &self.probe_pipeline, &photo.develop_group, [1, 1], 1);
        }
        if histogram {
            encoder.clear_buffer(&self.bins, 0, None);
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("histogram"), timestamp_writes: None });
            pass.set_pipeline(&self.histogram_pipeline);
            pass.set_bind_group(0, &photo.histogram_group, &[]);
            pass.dispatch_workgroups(photo.size[0].div_ceil(16), photo.size[1].div_ceil(16), 1);
        }
        if histogram {
            self.histogram_readback.copy_from(&mut encoder, &self.bins);
        }
        if probe.is_some() {
            self.probe_readback.copy_from(&mut encoder, &self.probed);
        }
        queue.submit([encoder.finish()]);

        if develop {
            self.developed = Some(edit.clone());
        }
        if histogram {
            self.histogram_stale = false;
            let (results, wake) = (self.results.clone(), self.wake.clone());
            self.histogram_readback.start(device, move |bytes| {
                let Some(bytes) = bytes else { return };
                // Read by value: a byte Vec isn't guaranteed to be aligned for u32.
                let counts: Vec<u32> = bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                if debug() {
                    let total: u64 = counts[..256].iter().map(|&c| c as u64).sum();
                    eprintln!("darkroom: histogram readback, {} bytes, {total} pixels", bytes.len());
                }
                let mut h = [[0u32; 256]; 4];
                for (c, row) in h.iter_mut().enumerate() {
                    row.copy_from_slice(&counts[c * 256..(c + 1) * 256]);
                }
                if let Ok(mut slot) = results.histogram.lock() {
                    *slot = Some(h);
                }
                wake();
            });
        }
        if let Some((id, _)) = probe {
            self.pending_probe = None;
            let (results, wake) = (self.results.clone(), self.wake.clone());
            self.probe_readback.start(device, move |bytes| {
                let Some(bytes) = bytes else { return };
                let v: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
                if debug() {
                    eprintln!("darkroom: probe readback {v:?}");
                }
                if let Ok(mut slot) = results.probe.lock() {
                    *slot = Some(Probed { id, rgb: [v[0], v[1], v[2]] });
                }
                wake();
            });
        }
    }

    /// Whether work is waiting on a busy readback, so the caller should call
    /// [`Self::update`] again on a later frame.
    pub fn has_waiting_work(&self) -> bool {
        self.histogram_stale || self.pending_probe.is_some()
    }

    /// Makes the present pipeline for `format` ahead of a pass that will need it.
    pub fn prepare_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if self.present_pipelines.contains_key(&format) {
            return;
        }
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: Some(&self.present_pipeline_layout),
            vertex: wgpu::VertexState { module: &self.present_module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.present_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.present_pipelines.insert(format, pipeline);
    }

    /// Draws the developed photo fitted (letterboxed) into `at.rect`. Needs
    /// [`Self::prepare_format`] for `at.format` first. Draws once per frame: the placement
    /// travels in one uniform.
    pub fn draw(&self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>, at: Placement) {
        let (Some(photo), Some(pipeline)) = (&self.photo, self.present_pipelines.get(&at.format)) else { return };
        let [x, y, w, h] = fit_aspect(at.rect, at.output);
        let (tw, th) = (at.target[0] as f32, at.target[1] as f32);
        let ndc = [x / tw * 2.0 - 1.0, 1.0 - y / th * 2.0, (x + w) / tw * 2.0 - 1.0, 1.0 - (y + h) / th * 2.0];
        let [a, b, c, d, e, f] = at.map;
        let [r, g, bl] = at.background;
        let uniform = PresentUniform {
            ndc,
            encode: [if at.encode_srgb { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            map_abcd: [a, b, c, d],
            map_ef: [e, f, 0.0, 0.0],
            background: [r, g, bl, 1.0],
        };
        queue.write_buffer(&self.present_uniform, 0, bytemuck::bytes_of(&uniform));
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &photo.present_group, &[]);
        pass.draw(0..4, 0..1);
    }
}

/// The definition blur's σ in quarter-size pixels: about 1% of the photo's width, so Definition
/// looks the same at any preview size.
pub fn definition_sigma(quarter: [u32; 2]) -> f32 {
    (quarter[0].max(quarter[1]) as f32 / 120.0).clamp(1.5, 10.0)
}

/// `DARKROOM_DEBUG=1` prints what readbacks return.
fn debug() -> bool {
    std::env::var_os("DARKROOM_DEBUG").is_some()
}

/// `size` scaled to fit inside `rect` and centred in it, snapped to whole pixels.
pub fn fit(rect: [f32; 4], size: [u32; 2]) -> [f32; 4] {
    fit_aspect(rect, size.map(|v| v as f32))
}

/// [`fit`] for a size given as floats (only its proportions matter).
pub fn fit_aspect(rect: [f32; 4], size: [f32; 2]) -> [f32; 4] {
    let [x, y, w, h] = rect;
    let k = (w / size[0]).min(h / size[1]);
    let (fw, fh) = ((size[0] * k).round(), (size[1] * k).round());
    [(x + (w - fw) / 2.0).round(), (y + (h - fh) / 2.0).round(), fw, fh]
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn a_wide_photo_letterboxes_into_a_tall_box() {
        assert_eq!(fit([10.0, 20.0, 300.0, 400.0], [3000, 2000]), [10.0, 120.0, 300.0, 200.0]);
    }

    #[test]
    fn a_tall_photo_pillarboxes_into_a_wide_box() {
        assert_eq!(fit([0.0, 0.0, 400.0, 300.0], [2000, 3000]), [100.0, 0.0, 200.0, 300.0]);
    }
}
