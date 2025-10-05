use crate::WgpuContextError;
use wgpu::{Device, TextureFormat, TextureUsages, TextureView};

/// Block on a future, polling the device as needed.
///
/// This will deadlock if the future is awaiting anything other than GPU progress.
#[cfg_attr(docsrs, doc(hidden))]
pub fn block_on_wgpu<F: Future>(device: &Device, fut: F) -> Result<F::Output, WgpuContextError> {
    if cfg!(target_arch = "wasm32") {
        panic!("WGPU is inherently async on WASM, so blocking doesn't work.");
    }

    // Dummy waker
    struct NullWake;
    impl std::task::Wake for NullWake {
        fn wake(self: std::sync::Arc<Self>) {}
    }

    // Create context to poll future with
    let waker = std::task::Waker::from(std::sync::Arc::new(NullWake));
    let mut context = std::task::Context::from_waker(&waker);

    // Same logic as `pin_mut!` macro from `pin_utils`.
    let mut fut = std::pin::pin!(fut);
    loop {
        match fut.as_mut().poll(&mut context) {
            std::task::Poll::Pending => {
                device.poll(wgpu::PollType::Wait)?;
            }
            std::task::Poll::Ready(item) => break Ok(item),
        }
    }
}

/// Create a WGPU Texture, returning a default TextureView
pub(crate) fn create_texture(
    width: u32,
    height: u32,
    format: TextureFormat,
    usage: TextureUsages,
    device: &Device,
) -> TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        usage,
        format,
        view_formats: &[],
    });

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
