//! Corral core: everything that does not need the network, root, or a GUI.

pub mod config;
pub mod drivers;
pub mod fsutil;
pub mod paths;
pub mod php;
pub mod sites;

pub use config::Config;
pub use paths::Paths;
