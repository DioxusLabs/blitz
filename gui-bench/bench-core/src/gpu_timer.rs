//! GPU timestamp queries, generated for the wgpu version an app's toolkit uses:
//!
//! ```ignore
//! bench_core::define_gpu_timer!(
//!     wgpu,
//!     |device: &wgpu::Device| {
//!         device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
//!     },
//!     |slice: wgpu::BufferSlice<'_>| slice.get_mapped_range().unwrap().to_vec()
//! );
//! ```
//!
//! The two closures cover API differences between wgpu versions: waiting for the device,
//! and reading a mapped buffer slice. The generated `GpuTimer` measures the GPU time between timestamp 0 and 1. Either give
//! a render pass `render_pass_writes()` (both ends of one pass), or bracket a toolkit's
//! work recorded into the same encoder with `mark_start` / `mark_end` (empty compute
//! passes). Bracketing only measures GPU work when both markers and the toolkit's work
//! are in one submission; across submissions the span also includes any time the GPU
//! sat idle waiting for the CPU.
//!
//! Needs `Features::TIMESTAMP_QUERY`. Adapters without it (Apple's paravirtual GPU in
//! macOS VMs, some software rasterizers) get `None` and the app omits `raster_gpu_us`.

#[macro_export]
macro_rules! define_gpu_timer {
    ($wgpu:ident, $poll_wait:expr, $read_mapped:expr $(,)?) => {
        pub struct GpuTimer {
            query_set: $wgpu::QuerySet,
            resolve: $wgpu::Buffer,
            readback: $wgpu::Buffer,
            period_ns: f64,
        }

        impl GpuTimer {
            pub fn new(device: &$wgpu::Device, queue: &$wgpu::Queue) -> Option<Self> {
                if !device.features().contains($wgpu::Features::TIMESTAMP_QUERY) {
                    return None;
                }
                let query_set = device.create_query_set(&$wgpu::QuerySetDescriptor {
                    label: Some("gui-bench timestamps"),
                    ty: $wgpu::QueryType::Timestamp,
                    count: 2,
                });
                let resolve = device.create_buffer(&$wgpu::BufferDescriptor {
                    label: Some("gui-bench timestamp resolve"),
                    size: 16,
                    usage: $wgpu::BufferUsages::QUERY_RESOLVE | $wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let readback = device.create_buffer(&$wgpu::BufferDescriptor {
                    label: Some("gui-bench timestamp readback"),
                    size: 16,
                    usage: $wgpu::BufferUsages::MAP_READ | $wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                Some(GpuTimer {
                    query_set,
                    resolve,
                    readback,
                    period_ns: queue.get_timestamp_period() as f64,
                })
            }

            pub fn render_pass_writes(&self) -> $wgpu::RenderPassTimestampWrites<'_> {
                $wgpu::RenderPassTimestampWrites {
                    query_set: &self.query_set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }
            }

            fn marker(&self, encoder: &mut $wgpu::CommandEncoder, index: u32) {
                let (begin, end) = if index == 0 {
                    (Some(0), None)
                } else {
                    (None, Some(1))
                };
                encoder.begin_compute_pass(&$wgpu::ComputePassDescriptor {
                    label: Some("gui-bench timestamp"),
                    timestamp_writes: Some($wgpu::ComputePassTimestampWrites {
                        query_set: &self.query_set,
                        beginning_of_pass_write_index: begin,
                        end_of_pass_write_index: end,
                    }),
                });
            }

            pub fn mark_start(&self, encoder: &mut $wgpu::CommandEncoder) {
                self.marker(encoder, 0);
            }

            /// Write the end timestamp and resolve both into the readback buffer.
            pub fn mark_end(&self, encoder: &mut $wgpu::CommandEncoder) {
                self.marker(encoder, 1);
                self.resolve(encoder);
            }

            /// Resolve the timestamps into the readback buffer (after the timed pass).
            pub fn resolve(&self, encoder: &mut $wgpu::CommandEncoder) {
                encoder.resolve_query_set(&self.query_set, 0..2, &self.resolve, 0);
                encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, 16u64);
            }

            /// Wait for the GPU and return the measured span in microseconds.
            pub fn read_us(&self, device: &$wgpu::Device) -> f64 {
                let slice = self.readback.slice(..);
                slice.map_async($wgpu::MapMode::Read, |_| {});
                ($poll_wait)(device);
                let (t0, t1) = {
                    let data: Vec<u8> = ($read_mapped)(slice);
                    (
                        u64::from_le_bytes(data[0..8].try_into().unwrap()),
                        u64::from_le_bytes(data[8..16].try_into().unwrap()),
                    )
                };
                self.readback.unmap();
                t1.saturating_sub(t0) as f64 * self.period_ns / 1000.0
            }
        }
    };
}
