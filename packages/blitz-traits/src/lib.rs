//! Types and traits to enable interoperability between the other Blitz crates without
//! circular or unnecessary dependencies.

pub mod devtools;
pub mod events;
pub mod navigation;
pub mod net;
pub mod shell;

pub use navigation::NavigationProvider;
pub use net::NetProvider;
pub use shell::ShellProvider;
