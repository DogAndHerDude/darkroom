//! Reading results back from the GPU without blocking a frame.
//!
//! A [`Readback`] owns one mappable buffer. After the work that fills it is submitted, `start`
//! maps it on a short-lived thread that waits on the device, copies the bytes out, and hands
//! them to a callback (which then wakes the UI). While one readback is in flight its buffer can't
//! be written, so callers check [`Readback::is_idle`] first and try again later if it's busy.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The longest a readback waits for the GPU before giving up.
const TIMEOUT: Duration = Duration::from_secs(2);

pub struct Readback {
    buffer: wgpu::Buffer,
    size: u64,
    busy: Arc<AtomicBool>,
}

impl Readback {
    pub fn new(device: &wgpu::Device, label: &'static str, size: u64) -> Readback {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Readback { buffer, size, busy: Arc::new(AtomicBool::new(false)) }
    }

    /// Whether the buffer is free to be written.
    pub fn is_idle(&self) -> bool {
        !self.busy.load(Ordering::Acquire)
    }

    /// Records a copy of `source` (from offset 0, this readback's size) into the buffer.
    pub fn copy_from(&self, encoder: &mut wgpu::CommandEncoder, source: &wgpu::Buffer) {
        encoder.copy_buffer_to_buffer(source, 0, &self.buffer, 0, self.size);
    }

    /// Records a copy of `rows` rows of `texture` from row `y`, each padded to
    /// `bytes_per_row` (a multiple of 256), into the buffer.
    pub fn copy_rows(&self, encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture, y: u32, rows: u32, bytes_per_row: u32) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &self.buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(rows) } },
            wgpu::Extent3d { width: texture.width(), height: rows, depth_or_array_layers: 1 },
        );
    }

    /// Maps the buffer once the GPU has run the submitted copy, then calls `done` with its bytes
    /// (None if the GPU never finished) from another thread. Call after submitting the encoder
    /// that holds the copy.
    pub fn start(&self, device: &wgpu::Device, done: impl FnOnce(Option<Vec<u8>>) + Send + 'static) {
        self.busy.store(true, Ordering::Release);
        let (device, buffer, busy) = (device.clone(), self.buffer.clone(), self.busy.clone());
        std::thread::spawn(move || {
            // Wait for the copy before mapping. RUNE's Metal backend may hold a submission back
            // until something flushes it, and maps "after the work submitted so far", which
            // wouldn't include a held copy; a blocking poll flushes and waits for it.
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            let mapped = Arc::new(AtomicBool::new(false));
            let failed = Arc::new(AtomicBool::new(false));
            {
                let (mapped, failed) = (mapped.clone(), failed.clone());
                // The callback only flips flags: wgpu calls must not be made from inside it.
                buffer.map_async(wgpu::MapMode::Read, .., move |r| match r {
                    Ok(()) => mapped.store(true, Ordering::Release),
                    Err(_) => failed.store(true, Ordering::Release),
                });
            }
            let start = Instant::now();
            while !mapped.load(Ordering::Acquire) && !failed.load(Ordering::Acquire) && start.elapsed() < TIMEOUT {
                // Waits for the latest submission and runs its callbacks; poll again in case
                // the backend finished the map on its own schedule.
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if !mapped.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            if mapped.load(Ordering::Acquire) {
                let bytes = buffer.get_mapped_range(..).map(|view| view.to_vec()).unwrap_or_default();
                buffer.unmap();
                busy.store(false, Ordering::Release);
                done((!bytes.is_empty()).then_some(bytes));
            } else {
                // Never mapped: leave the buffer busy rather than risk writing a pending map.
                // The next device (or app start) makes a new one.
                eprintln!("darkroom: a GPU readback didn't finish within {TIMEOUT:?}");
                done(None);
            }
        });
    }
}
