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
use clack_extensions::gui::{GuiSize, HostGui, HostGuiImpl};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::note_ports::{HostNotePortsImpl, NoteDialects, NotePortRescanFlags};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
};
use clack_host::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Top-level host type registered with `clack-host`.
///
/// Associates the three handler lifetimes required by the framework.
pub struct MidiPlayerHost;

impl HostHandlers for MidiPlayerHost {
    type Shared<'a> = MidiPlayerHostShared;
    type MainThread<'a> = MidiPlayerHostMainThread<'a>;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostParams>()
            .register::<HostGui>();
    }
}

/// Shared (thread-safe) host state.
///
/// Holds an atomic flag that signals when the plugin has requested a
/// main-thread callback via `request_callback`.  The main GTK loop must
/// periodically check this flag and, if set, invoke
/// `PluginInstance::call_on_main_thread_callback()`.
pub struct MidiPlayerHostShared {
    /// Set to `true` by the plugin (from any thread) via `request_callback`.
    callback_requested: Arc<AtomicBool>,
}

impl MidiPlayerHostShared {
    /// Create a new shared host state instance.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            callback_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a new instance whose callback flag is backed by `flag`.
    ///
    /// This allows external code (e.g. `ClapPluginGuiHandle`) to read the
    /// same `AtomicBool` without going through `HostWrapper`.
    pub fn with_shared_flag(flag: Arc<AtomicBool>) -> Self {
        Self {
            callback_requested: flag,
        }
    }

    /// Returns a cloneable handle to the callback flag.
    #[allow(dead_code)]
    pub fn callback_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.callback_requested)
    }
}

impl<'a> SharedHandler<'a> for MidiPlayerHostShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::SeqCst);
    }
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
        NoteDialects::CLAP | NoteDialects::MIDI
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

impl HostGuiImpl for MidiPlayerHostShared {
    fn resize_hints_changed(&self) {
        // Floating window: plugin handles its own sizing.
    }

    fn request_resize(&self, _new_size: GuiSize) -> Result<(), HostError> {
        // Floating window: plugin manages its own window size.
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn closed(&self, _was_destroyed: bool) {
        // TODO: notify UI that the plugin GUI was closed.
        eprintln!("[CLAP GUI] Plugin GUI closed");
    }
}
