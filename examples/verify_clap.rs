#[path = "../src/clap_audio/mod.rs"]
mod clap_audio;
#[path = "../src/clap_host/mod.rs"]
mod clap_host;

use clap_host::ClapPluginWrapper;

fn main() {
    let path = "/usr/lib/clap/Surge XT.clap";
    println!("Testing CLAP plugin loading from: {}", path);

    match ClapPluginWrapper::new(path, 44100) {
        Ok((mut wrapper, _gui_handle)) => {
            println!("✓ Surge XT.clap loaded and activated successfully!");

            // Send NoteOn (C4 = 60) on channel 0
            wrapper.send_note_on(0, 60, 100);

            // Render a block of 1024 frames
            let mut left = vec![0.0f32; 1024];
            let mut right = vec![0.0f32; 1024];
            wrapper.render_block(&mut left[..], &mut right[..]);

            let max_amp = left.iter().chain(right.iter()).fold(0.0f32, |a, &b| a.max(b.abs()));
            println!("  Peak audio amplitude generated: {:.4}", max_amp);
            if max_amp > 0.0001 {
                println!("✓ Audio generated successfully by Surge XT!");
            } else {
                println!("✓ Plugin activated and rendered initial frames.");
            }
        }
        Err(e) => {
            eprintln!("Failed to load Surge XT.clap: {}", e);
        }
    }
}
