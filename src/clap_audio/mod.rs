//! CLAP audio infrastructure.
//!
//! This module provides the low-level audio buffer management and configuration
//! types needed to feed audio data between the CPAL output stream and a CLAP
//! plugin's `process()` call.
//!
//! * [`buffers`] – [`HostAudioBuffers`](buffers::HostAudioBuffers) handles
//!   allocation, interleaving/de-interleaving, and channel muxing.
//! * [`config`] – [`FullAudioConfig`](config::FullAudioConfig) describes the
//!   negotiated audio format (sample rate, buffer size, port layout).

pub mod buffers;
pub mod config;
