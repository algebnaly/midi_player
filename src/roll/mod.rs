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
pub mod view;
pub mod viewport;

#[allow(unused_imports)]
pub use layout::{DrumLayout, MelodicLayout, RollLayout};
#[allow(unused_imports)]
pub use state::RollState;
#[allow(unused_imports)]
pub use types::*;
pub use view::RollView;
#[allow(unused_imports)]
pub use viewport::Viewport;
