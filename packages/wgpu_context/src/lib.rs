// Copyright 2022 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Simple helpers for managing wgpu state and surfaces.

use std::future::Future;
use wgpu::{
    Adapter, CommandEncoderDescriptor, Device, Features, Instance, Limits, MemoryHints, Queue,
    Surface, SurfaceConfiguration, SurfaceTarget, SurfaceTexture, TextureFormat, TextureView,
    TextureViewDescriptor, util::TextureBlitter,
};

mod error;
pub use error::WgpuContextError;

/// Simple render context that maintains wgpu state for rendering the pipeline.
pub struct WGPUContext {
    /// A WGPU `Instance`. This only needs to be created once per application.
    pub instance: Instance,
    /// A pool of already-created devices so that we can avoid recreating devices
    /// when we already have a suitable one available
    pub device_pool: Vec<DeviceHandle>,

    // Config
    extra_features: Option<Features>,
    override_limits: Option<Limits>,
}

/// A wgpu `Device`, it's associated `Queue`, and the `Adapter` and `Instance` used to create them
#[derive(Clone, Debug)]
pub struct DeviceHandle {
    pub instance: Instance,
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
}

impl WGPUContext {
    pub fn new() -> Self {
        Self::with_features_and_limits(None, None)
    }

    pub fn with_features_and_limits(
        extra_features: Option<Features>,
        override_limits: Option<Limits>,
    ) -> Self {
        Self {
            instance: Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::from_env().unwrap_or_default(),
                flags: wgpu::InstanceFlags::from_build_config().with_env(),
                backend_options: wgpu::BackendOptions::from_env_or_default(),
                memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            }),
            device_pool: Vec::new(),
            extra_features,
            override_limits,
        }
    }

    /// Creates a new surface for the specified window and dimensions.
    pub async fn create_surface<'w>(
        &mut self,
        window: impl Into<SurfaceTarget<'w>>,
        width: u32,
        height: u32,
        present_mode: wgpu::PresentMode,
    ) -> Result<SurfaceRenderer<'w>, WgpuContextError> {
        // Create a surface from the window handle
        let surface = self.instance.create_surface(window.into())?;

        // Find or create a suitable device for rendering to the surface
        let dev_id = self
            .find_or_create_device(Some(&surface))
            .await
            .or(Err(WgpuContextError::NoCompatibleDevice))?;
        let device_handle = self.device_pool[dev_id].clone();

        SurfaceRenderer::new(surface, device_handle, dev_id, width, height, present_mode).await
    }

    /// Finds or creates a compatible device handle id.
    pub async fn find_or_create_device(
        &mut self,
        compatible_surface: Option<&Surface<'_>>,
    ) -> Result<usize, WgpuContextError> {
        match self.find_existing_device(compatible_surface) {
            Some(device_id) => Ok(device_id),
            None => self.create_device(compatible_surface).await,
        }
    }

    /// Finds or creates a compatible device handle id.
    fn find_existing_device(&self, compatible_surface: Option<&Surface<'_>>) -> Option<usize> {
        match compatible_surface {
            Some(s) => self
                .device_pool
                .iter()
                .enumerate()
                .find(|(_, d)| d.adapter.is_surface_supported(s))
                .map(|(i, _)| i),
            None => (!self.device_pool.is_empty()).then_some(0),
        }
    }

    /// Creates a compatible device handle id.
    async fn create_device(
        &mut self,
        compatible_surface: Option<&Surface<'_>>,
    ) -> Result<usize, WgpuContextError> {
        let instance = self.instance.clone();
        let adapter =
            wgpu::util::initialize_adapter_from_env_or_default(&instance, compatible_surface)
                .await?;

        // Determine features to request
        // The user may request additional features
        let requested_features = self.extra_features.unwrap_or(Features::empty());
        let available_features = adapter.features();
        let required_features = requested_features & available_features;

        // Determine limits to request
        // The user may override the limits
        let required_limits = self.override_limits.clone().unwrap_or_default();

        // Create the device and the queue
        let descripter = wgpu::DeviceDescriptor {
            label: None,
            required_features,
            required_limits,
            memory_hints: MemoryHints::default(),
            trace: wgpu::Trace::default(),
        };
        let (device, queue) = adapter.request_device(&descripter).await?;

        // Create the device handle and store in the pool
        let device_handle = DeviceHandle {
            instance,
            adapter,
            device,
            queue,
        };
        self.device_pool.push(device_handle);

        // Return the ID
        Ok(self.device_pool.len() - 1)
    }
}

/// Vello uses a compute shader to render to the provided texture, which means that it can't bind the surface
/// texture in most cases.
///
/// Because of this, we need to create an "intermediate" texture which we render to, and then blit to the surface.
fn create_intermediate_texture(width: u32, height: u32, device: &Device) -> TextureView {
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
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        format: TextureFormat::Rgba8Unorm,
        view_formats: &[],
    });

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

impl Default for WGPUContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Combination of surface and its configuration.
pub struct SurfaceRenderer<'s> {
    // The device and queue for rendering to the surface
    pub dev_id: usize,
    pub device_handle: DeviceHandle,

    // The surface and it's configuration
    pub surface: Surface<'s>,
    pub config: SurfaceConfiguration,

    // TextureView for the intermediate Texture which we sometimes render to because compute shaders
    // cannot always render directly to surfaces. Since WGPU 26, the underlying Texture can be accessed
    // from the TextureView so we don't need to store both.
    pub intermediate_texture_view: TextureView,
    // Blitter for blitting from the intermediate texture to the surface.
    pub blitter: TextureBlitter,
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
        device_handle: DeviceHandle,
        dev_id: usize,
        width: u32,
        height: u32,
        present_mode: wgpu::PresentMode,
    ) -> Result<SurfaceRenderer<'w>, WgpuContextError> {
        let capabilities = surface.get_capabilities(&device_handle.adapter);
        let format = capabilities
            .formats
            .into_iter()
            .find(|it| matches!(it, TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm))
            .ok_or(WgpuContextError::UnsupportedSurfaceFormat)?;

        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        let intermediate_texture_view =
            create_intermediate_texture(width, height, &device_handle.device);
        let blitter = TextureBlitter::new(&device_handle.device, format);
        let surface = SurfaceRenderer {
            dev_id,
            device_handle,
            surface,
            config,
            intermediate_texture_view,
            blitter,
        };
        surface.configure();
        Ok(surface)
    }

    /// Resizes the surface to the new dimensions.
    pub fn resize(&mut self, width: u32, height: u32) {
        let texture_view = create_intermediate_texture(width, height, &self.device_handle.device);
        // TODO: Use clever resize semantics to avoid thrashing the memory allocator during a resize
        // especially important on metal.
        self.intermediate_texture_view = texture_view;
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

    /// Blit from the intermediate texture to the surface texture
    pub fn blit_from_intermediate_texture_to_surface(&self) -> SurfaceTexture {
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");

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

        self.blitter.copy(
            &self.device_handle.device,
            &mut encoder,
            &self.intermediate_texture_view,
            &surface_texture
                .texture
                .create_view(&TextureViewDescriptor::default()),
        );
        self.device_handle.queue.submit([encoder.finish()]);

        surface_texture
    }
}

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
