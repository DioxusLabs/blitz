#[cfg(feature = "softbuffer_window_renderer")]
#[cfg_attr(docsrs, doc(cfg(feature = "softbuffer_window_renderer")))]
mod softbuffer_window_renderer;
#[cfg(feature = "softbuffer_window_renderer")]
pub use softbuffer_window_renderer::VelloCpuSoftbufferWindowRenderer;
#[cfg(feature = "pixels_window_renderer")]
#[cfg_attr(docsrs, doc(cfg(feature = "pixels_window_renderer")))]
mod pixels_window_renderer;
#[cfg(feature = "pixels_window_renderer")]
pub use pixels_window_renderer::VelloCpuPixelsWindowRenderer;

#[cfg(feature = "pixels_window_renderer")]
pub use VelloCpuPixelsWindowRenderer as VelloCpuWindowRenderer;
#[cfg(all(
    feature = "softbuffer_window_renderer",
    not(feature = "pixels_window_renderer")
))]
pub use VelloCpuSoftbufferWindowRenderer as VelloCpuWindowRenderer;
