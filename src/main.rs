mod midi;
mod piano_roll;
mod player;
mod window;

use gtk::Application;
use gtk::prelude::*;
use gtk4 as gtk;

fn main() {
    let app = Application::builder()
        .application_id("com.github.midiplayer")
        .build();

    app.connect_activate(window::build_ui);

    app.run();
}
