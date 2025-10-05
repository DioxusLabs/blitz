use crate::{DeviceHandle, WgpuContextError};
use wgpu::{
    CommandEncoderDescriptor, CompositeAlphaMode, Device, PresentMode, Surface,
    SurfaceConfiguration, SurfaceTexture, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, util::TextureBlitter,
};

/// Vello uses a compute shader to render to the provided texture, which means that it can't bind the surface
/// texture in most cases.
///
/// Because of this, we need to create an "intermediate" texture which we render to, and then blit to the surface.
fn create_intermediate_texture(
    width: u32,
    height: u32,
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
        format: TextureFormat::Rgba8Unorm,
        view_formats: &[],
    });

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

#[derive(Clone)]
pub struct TextureConfiguration {
    pub usage: TextureUsages,
}

#[derive(Clone)]
pub struct SurfaceRendererConfiguration {
    /// The usage of the swap chain. The only usage guaranteed to be supported is [`TextureUsages::RENDER_ATTACHMENT`].
    pub usage: TextureUsages,
    /// The texture format of the swap chain. The only formats that are guaranteed are
    /// [`TextureFormat::Bgra8Unorm`] and [`TextureFormat::Bgra8UnormSrgb`].
    pub formats: Vec<TextureFormat>,
    /// Width of the swap chain. Must be the same size as the surface, and nonzero.
    ///
    /// If this is not the same size as the underlying surface (e.g. if it is
    /// set once, and the window is later resized), the behaviour is defined
    /// but platform-specific, and may change in the future (currently macOS
    /// scales the surface, other platforms may do something else).
    pub width: u32,
    /// Height of the swap chain. Must be the same size as the surface, and nonzero.
    ///
    /// If this is not the same size as the underlying surface (e.g. if it is
    /// set once, and the window is later resized), the behaviour is defined
    /// but platform-specific, and may change in the future (currently macOS
    /// scales the surface, other platforms may do something else).
    pub height: u32,
    /// Presentation mode of the swap chain. Fifo is the only mode guaranteed to be supported.
    /// `FifoRelaxed`, `Immediate`, and `Mailbox` will crash if unsupported, while `AutoVsync` and
    /// `AutoNoVsync` will gracefully do a designed sets of fallbacks if their primary modes are
    /// unsupported.
    pub present_mode: PresentMode,
    /// Desired maximum number of frames that the presentation engine should queue in advance.
    ///
    /// This is a hint to the backend implementation and will always be clamped to the supported range.
    /// As a consequence, either the maximum frame latency is set directly on the swap chain,
    /// or waits on present are scheduled to avoid exceeding the maximum frame latency if supported,
    /// or the swap chain size is set to (max-latency + 1).
    ///
    /// Defaults to 2 when created via `Surface::get_default_config`.
    ///
    /// Typical values range from 3 to 1, but higher values are possible:
    /// * Choose 2 or higher for potentially smoother frame display, as it allows to be at least one frame
    ///   to be queued up. This typically avoids starving the GPU's work queue.
    ///   Higher values are useful for achieving a constant flow of frames to the display under varying load.
    /// * Choose 1 for low latency from frame recording to frame display.
    ///   ⚠️ If the backend does not support waiting on present, this will cause the CPU to wait for the GPU
    ///   to finish all work related to the previous frame when calling `Surface::get_current_texture`,
    ///   causing CPU-GPU serialization (i.e. when `Surface::get_current_texture` returns, the GPU might be idle).
    ///   It is currently not possible to query this. See <https://github.com/gfx-rs/wgpu/issues/2869>.
    /// * A value of 0 is generally not supported and always clamped to a higher value.
    pub desired_maximum_frame_latency: u32,
    /// Specifies how the alpha channel of the textures should be handled during compositing.
    pub alpha_mode: CompositeAlphaMode,
    /// Specifies what view formats will be allowed when calling `Texture::create_view` on the texture returned by `Surface::get_current_texture`.
    ///
    /// View formats of the same format as the texture are always allowed.
    ///
    /// Note: currently, only the srgb-ness is allowed to change. (ex: `Rgba8Unorm` texture + `Rgba8UnormSrgb` view)
    pub view_formats: Vec<TextureFormat>,
}

struct IntermediateTextureStuff {
    pub config: TextureConfiguration,
    // TextureView for the intermediate Texture which we sometimes render to because compute shaders
    // cannot always render directly to surfaces. Since WGPU 26, the underlying Texture can be accessed
    // from the TextureView so we don't need to store both.
    pub texture_view: TextureView,
    // Blitter for blitting from the intermediate texture to the surface.
    pub blitter: TextureBlitter,
}

/// Combination of surface and its configuration.
pub struct SurfaceRenderer<'s> {
    // The device and queue for rendering to the surface
    pub dev_id: usize,
    pub device_handle: DeviceHandle,

    // The surface and it's configuration
    pub surface: Surface<'s>,
    pub config: SurfaceConfiguration,

    intermediate_texture: Option<Box<IntermediateTextureStuff>>,
}

impl std::fmt::Debug for SurfaceRenderer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceRenderer")
            .field("dev_id", &self.dev_id)
            .field("surface_config", &self.config)
            .field("has_intermediate_texture", &true)
            .finish()
    }
}

impl<'s> SurfaceRenderer<'s> {
    /// Creates a new render surface for the specified window and dimensions.
    pub async fn new<'w>(
        surface: Surface<'w>,
        surface_renderer_config: SurfaceRendererConfiguration,
        intermediate_texture_config: Option<TextureConfiguration>,
        device_handle: DeviceHandle,
        dev_id: usize,
    ) -> Result<SurfaceRenderer<'w>, WgpuContextError> {
        // Convert SurfaceRendererConfiguration to SurfaceConfiguration.
        // The difference is that `format` is a Vec in SurfaceRendererConfiguration and a single value in SurfaceConfiguration
        let surface_config = SurfaceConfiguration {
            usage: surface_renderer_config.usage,
            format: surface
                .get_capabilities(&device_handle.adapter)
                .formats
                .into_iter()
                .find(|it| surface_renderer_config.formats.contains(it))
                .ok_or(WgpuContextError::UnsupportedSurfaceFormat)?,
            width: surface_renderer_config.width,
            height: surface_renderer_config.height,
            present_mode: surface_renderer_config.present_mode,
            desired_maximum_frame_latency: surface_renderer_config.desired_maximum_frame_latency,
            alpha_mode: surface_renderer_config.alpha_mode,
            view_formats: surface_renderer_config.view_formats,
        };

        let intermediate_texture = intermediate_texture_config.map(|texture_config| {
            Box::new(IntermediateTextureStuff {
                config: texture_config.clone(),
                texture_view: create_intermediate_texture(
                    surface_renderer_config.width,
                    surface_renderer_config.height,
                    texture_config.usage,
                    &device_handle.device,
                ),
                blitter: TextureBlitter::new(&device_handle.device, surface_config.format),
            })
        });

        let surface = SurfaceRenderer {
            dev_id,
            device_handle,
            surface,
            config: surface_config,
            intermediate_texture,
        };
        surface.configure();
        Ok(surface)
    }

    /// Resizes the surface to the new dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        // TODO: Use clever resize semantics to avoid thrashing the memory allocator during a resize
        // especially important on metal.
        if let Some(intermediate_texture_stuff) = &mut self.intermediate_texture {
            intermediate_texture_stuff.texture_view = create_intermediate_texture(
                width,
                height,
                intermediate_texture_stuff.config.usage,
                &self.device_handle.device,
            );
        }
        self.config.width = width;
        self.config.height = height;
        self.configure();
    }

    pub fn set_present_mode(&mut self, present_mode: wgpu::PresentMode) {
        self.config.present_mode = present_mode;
        self.configure();
    }

    fn configure(&self) {
        self.surface
            .configure(&self.device_handle.device, &self.config);
    }

    pub fn target_texture_view(&self) -> TextureView {
        match &self.intermediate_texture {
            Some(intermediate_texture) => intermediate_texture.texture_view.clone(),
            None => {
                let surface_texture = self
                    .surface
                    .get_current_texture()
                    .expect("failed to get surface texture");
                surface_texture
                    .texture
                    .create_view(&TextureViewDescriptor::default())
            }
        }
    }

    pub fn maybe_blit_and_present(&self) {
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");

        if let Some(its) = &self.intermediate_texture {
            self.blit_from_intermediate_texture_to_surface(&surface_texture, its);
        }

        surface_texture.present();
    }

    /// Blit from the intermediate texture to the surface texture
    fn blit_from_intermediate_texture_to_surface(
        &self,
        surface_texture: &SurfaceTexture,
        intermediate_texture_stuff: &IntermediateTextureStuff,
    ) {
        // TODO: verify that handling of SurfaceError::Outdated is no longer required
        //
        // let surface_texture = match state.surface.surface.get_current_texture() {
        //     Ok(surface) => surface,
        //     // When resizing too aggresively, the surface can get outdated (another resize) before being rendered into
        //     Err(SurfaceError::Outdated) => return,
        //     Err(_) => panic!("failed to get surface texture"),
        // };

        // Perform the copy
        // (TODO: Does it improve throughput to acquire the surface after the previous texture render has happened?)
        let mut encoder =
            self.device_handle
                .device
                .create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("Surface Blit"),
                });

        intermediate_texture_stuff.blitter.copy(
            &self.device_handle.device,
            &mut encoder,
            &intermediate_texture_stuff.texture_view,
            &surface_texture
                .texture
                .create_view(&TextureViewDescriptor::default()),
        );
        self.device_handle.queue.submit([encoder.finish()]);
    }
}
