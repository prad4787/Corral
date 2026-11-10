//! Corral core: everything that does not need the network, root, or a GUI.

pub mod config;
pub mod dotenv;
pub mod drivers;
pub mod fsutil;
pub mod logs;
pub mod paths;
pub mod php;
pub mod shell;
pub mod sites;

pub use config::Config;
pub use paths::Paths;
