# Rust MIDI Player & Editor

A GTK4-based MIDI piano-roll editor with multi-track playback support for both
SoundFont (`.sf2`) and CLAP plugin (`.clap`) synthesizers.

## Architecture

```
src/
├── main.rs              # Application entry point
├── config.rs            # User configuration (~/.config/midi_player/)
├── window.rs            # GTK4 window construction & event wiring
├── piano_roll.rs        # Custom piano-roll widget (GObject subclass)
├── player.rs            # High-level playback controller (facade)
├── audio_engine.rs      # CPAL device discovery & audio stream
├── sequencer.rs         # Sample-accurate MIDI event scheduler (auto-loop)
├── synth/               # Synthesizer abstraction layer
│   └── mod.rs           #   TrackSynth enum (SoundFont | ClapPlugin)
├── midi.rs              # MIDI data model, file I/O, event compilation
├── clap_host/           # CLAP plugin hosting (clack-based)
│   ├── mod.rs           #   Module root & re-exports
│   ├── host.rs          #   Host handler types & extension impls
│   └── wrapper.rs       #   ClapPluginWrapper lifecycle management
└── clap_audio/          # Low-level CLAP audio infrastructure
    ├── mod.rs
    ├── buffers.rs       #   Host audio buffer management & muxing
    └── config.rs        #   Audio configuration types & negotiation
```

## Features

- **Piano Roll Editor** – place, move, and resize MIDI notes on a grid with
  snap-to-grid quantization.
- **Multi-Track Playback** – simultaneous SoundFont + CLAP plugin rendering
  with additive mixing.
- **CLAP Plugin Hosting** – load any CLAP instrument plugin from a `.clap`
  bundle; auto-selects the first instrument descriptor.
- **Live Note Preview** – click on the piano roll to audition notes through
  the selected track's synth engine.
- **Hot-Swap Editing** – change BPM or edit notes while playing without
  interrupting the audio stream.
- **MIDI File I/O** – open and export Standard MIDI Files (`.mid`).

## Dependencies

| Crate              | Purpose                             |
|--------------------|-------------------------------------|
| `gtk4`             | UI framework (piano roll, window)   |
| `cpal`             | Cross-platform audio output         |
| `oxisynth`         | SoundFont (`.sf2`) synthesis        |
| `clack-host`       | CLAP plugin hosting                 |
| `clack-extensions` | CLAP extension implementations      |
| `midly`            | MIDI file parsing / writing         |
| `serde` + `toml`   | Configuration file parsing          |
| `anyhow`           | Error handling                      |

## Building

```bash
# Build the DAW
cargo build --release
```

On Windows, set `GtkRoot` to your GTK4 release directory, then make
`pkg-config`, the GTK DLLs, and the MSVC import libraries visible before
building:

```powershell
$GtkRoot = "C:\gtk-build\gtk\x64\release"
$env:Path = "$GtkRoot\bin;" + $env:Path
$env:PKG_CONFIG_PATH = "$GtkRoot\lib\pkgconfig"
$env:RUSTFLAGS = "-L native=$GtkRoot\lib"
cargo build --release
```

If GTK is installed somewhere else, change only `$GtkRoot`.


## Configuration

On first run a default config file is created at
`~/.config/midi_player/config.toml`:

```toml
default_bpm = 120.0          # BPM for new projects
default_note_beats = 1.0     # Note duration in beats (1.0 = quarter, 0.125 = 32nd)
soundfont_path = "default.sf2"
clap_plugin_path = "./plugin.clap"
```

## Running

1. Place a SoundFont file at the path specified by `soundfont_path` in the
   config (default: `default.sf2` in the project root).
2. Optionally place a `.clap` plugin at the `clap_plugin_path` location.
3. Run:

```bash
cargo run
```

## Key Bindings

| Key     | Action            |
|---------|-------------------|
| `Space` | Play / Pause      |

## License

See repository for license details.
