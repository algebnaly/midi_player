//! CLAP host handler types and extension implementations.
//!
//! Defines [`MidiPlayerHost`], [`MidiPlayerHostShared`], and
//! [`MidiPlayerHostMainThread`] – the three handler types required by
//! the `clack-host` framework to manage a plugin instance.
//!
//! Also implements the following CLAP extensions on behalf of the host:
//! * **Log** – forwards plugin log messages to stderr.
//! * **Audio Ports** – stub (no rescan support).
//! * **Note Ports** – advertises CLAP note dialect.
//! * **Params** – stub (no parameter rescan/clear support).

use clack_extensions::audio_ports::{AudioPortRescanFlags, HostAudioPortsImpl};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::note_ports::{HostNotePortsImpl, NoteDialects, NotePortRescanFlags};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
};
use clack_host::prelude::*;

/// Top-level host type registered with `clack-host`.
///
/// Associates the three handler lifetimes required by the framework.
pub struct MidiPlayerHost;

impl HostHandlers for MidiPlayerHost {
    type Shared<'a> = MidiPlayerHostShared;
    type MainThread<'a> = MidiPlayerHostMainThread<'a>;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder.register::<HostLog>().register::<HostParams>();
    }
}

/// Shared (thread-safe) host state.
///
/// Currently stateless – exists only to satisfy the `clack-host` trait bounds
/// and to implement shared extension handlers such as logging.
pub struct MidiPlayerHostShared;

impl MidiPlayerHostShared {
    /// Create a new shared host state instance.
    pub fn new() -> Self {
        Self
    }
}

impl<'a> SharedHandler<'a> for MidiPlayerHostShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

/// Main-thread host state, holding a reference to the shared state.
pub struct MidiPlayerHostMainThread<'a> {
    _shared: &'a MidiPlayerHostShared,
}

impl<'a> MidiPlayerHostMainThread<'a> {
    /// Create a new main-thread handler with a reference to shared state.
    pub fn new(shared: &'a MidiPlayerHostShared) -> Self {
        Self { _shared: shared }
    }
}

impl<'a> MainThreadHandler<'a> for MidiPlayerHostMainThread<'a> {}

/// Returns the [`HostInfo`] metadata advertised to plugins.
pub fn host_info() -> HostInfo {
    HostInfo::new(
        "MIDI Player CLAP Host",
        "MIDI Player",
        "https://github.com/algebnaly/midi_player",
        "0.1.0",
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Extension implementations
// ---------------------------------------------------------------------------

impl HostLogImpl for MidiPlayerHostShared {
    fn log(&self, severity: LogSeverity, message: &str) {
        if severity <= LogSeverity::Debug {
            return;
        }
        eprintln!("[CLAP {}] {}", severity.to_string(), message);
    }
}

impl HostAudioPortsImpl for MidiPlayerHostMainThread<'_> {
    fn is_rescan_flag_supported(&self, _flag: AudioPortRescanFlags) -> bool {
        false
    }
    fn rescan(&mut self, _flags: AudioPortRescanFlags) {}
}

impl HostNotePortsImpl for MidiPlayerHostMainThread<'_> {
    fn supported_dialects(&self) -> NoteDialects {
        NoteDialects::CLAP
    }
    fn rescan(&mut self, _flags: NotePortRescanFlags) {}
}

impl HostParamsImplMainThread for MidiPlayerHostMainThread<'_> {
    fn rescan(&mut self, _flags: ParamRescanFlags) {}
    fn clear(&mut self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

impl HostParamsImplShared for MidiPlayerHostShared {
    fn request_flush(&self) {}
}
