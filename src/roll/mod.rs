//! Shared roll core used by the piano and drum Views.
//!
//! This module is not a widget. Each View (`piano_roll`, `drum_roll`) owns
//! drawing and the left-hand keyboard; editing state and input live here.

pub mod input;
pub mod keys;
pub mod layout;
pub mod renderer;
pub mod state;
pub mod types;
pub mod viewport;
pub mod view;

#[allow(unused_imports)]
pub use layout::{DrumLayout, MelodicLayout, RollLayout};
#[allow(unused_imports)]
pub use state::RollState;
#[allow(unused_imports)]
pub use types::*;
#[allow(unused_imports)]
pub use viewport::Viewport;
pub use view::RollView;
