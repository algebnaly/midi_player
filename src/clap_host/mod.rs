//! CLAP plugin host implementation.
//!
//! This module provides everything needed to load, activate, and process audio
//! through external [CLAP](https://cleveraudio.org/) plugins.
//!
//! # Sub-modules
//!
//! * [`host`] – Host handler types, extension implementations, and host metadata.
//! * [`wrapper`] – [`ClapPluginWrapper`] (audio thread) and [`ClapPluginGuiHandle`]
//!   (main thread) which together manage the full plugin lifecycle.

mod host;
mod wrapper;

pub use host::*;
pub use wrapper::{ClapPluginGuiHandle, ClapPluginWrapper};
