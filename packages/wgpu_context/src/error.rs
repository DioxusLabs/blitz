//! Error type for WGPU Context

use std::error::Error;
use std::fmt::Display;
use wgpu::{PollError, RequestAdapterError, RequestDeviceError};

/// Errors that can occur in WgpuContext.
#[derive(Debug)]
pub enum WgpuContextError {
    /// There is no available device with the features required by Vello.
    NoCompatibleDevice,
    /// Failed to create surface.
    /// See [`wgpu::CreateSurfaceError`] for more information.
    WgpuCreateSurfaceError(wgpu::CreateSurfaceError),
    /// Surface doesn't support the required texture formats.
    /// Make sure that you have a surface which provides one of
    /// `TextureFormat::Rgba8Unorm`
    /// or [`TextureFormat::Bgra8Unorm`] as texture formats.
    // TODO: Why does this restriction exist?
    UnsupportedSurfaceFormat,
    /// Wgpu failed to request an adapter
    RequestAdapterError(RequestAdapterError),
    /// Wgpu failed to request a device
    RequestDeviceError(RequestDeviceError),
    /// Wgpu failed to poll a device
    PollError(PollError),
}

impl Display for WgpuContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCompatibleDevice => writeln!(f, "Couldn't find suitable device"),
            Self::WgpuCreateSurfaceError(inner) => {
                writeln!(f, "Couldn't create wgpu surface")?;
                inner.fmt(f)
            }
            Self::UnsupportedSurfaceFormat => {
                writeln!(
                    f,
                    "Couldn't find `Rgba8Unorm` or `Bgra8Unorm` texture formats for surface"
                )
            }
            Self::RequestAdapterError(inner) => {
                writeln!(f, "Couldn't request an adapter: {:#}", inner)
            }
            Self::RequestDeviceError(inner) => {
                writeln!(f, "Couldn't request a device: {:#}", inner)
            }
            Self::PollError(inner) => {
                writeln!(f, "Couldn't poll a device: {:#}", inner)
            }
        }
    }
}

impl Error for WgpuContextError {}
impl From<wgpu::CreateSurfaceError> for WgpuContextError {
    fn from(value: wgpu::CreateSurfaceError) -> Self {
        Self::WgpuCreateSurfaceError(value)
    }
}

impl From<RequestAdapterError> for WgpuContextError {
    fn from(value: RequestAdapterError) -> Self {
        Self::RequestAdapterError(value)
    }
}

impl From<RequestDeviceError> for WgpuContextError {
    fn from(value: RequestDeviceError) -> Self {
        Self::RequestDeviceError(value)
    }
}

impl From<PollError> for WgpuContextError {
    fn from(value: PollError) -> Self {
        Self::PollError(value)
    }
}
