#[cfg(feature = "blocking")]
mod blocking;
mod dummy;
#[cfg(feature = "non_blocking")]
mod non_blocking;

#[cfg(feature = "non_blocking")]
pub use non_blocking::AsyncProvider;

#[cfg(feature = "blocking")]
pub use blocking::SyncProvider;

pub use dummy::DummyProvider;
