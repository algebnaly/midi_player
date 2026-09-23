//! Application stylesheet for the status bar and floating panels.

use gtk4 as gtk;

pub fn apply_stylesheet() {
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_data(
        ".status-bar { font-family: monospace; font-size: 14px; font-weight: bold; \
         padding: 6px 10px; background: #1a1a1a; color: #eee; \
         min-height: 22px; } \
         .floating-panel { padding: 12px; color: #eeeeee; \
         background: rgba(26, 26, 26, 0.95); border: 1px solid #3a3a3a; \
         border-radius: 10px; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.6); } \
         .floating-panel label { color: #eeeeee; } \
         .floating-panel .panel-title { color: #ffffff; font-size: 16px; font-weight: 700; } \
         .floating-panel .panel-hint { color: #a0a0a0; font-size: 13px; } \
         .floating-panel button { color: #eeeeee; background-color: #333333; background-image: none; \
         border: 1px solid #4a4a4a; border-radius: 6px; box-shadow: none; padding: 5px 8px; } \
         .floating-panel button label { color: #eeeeee; } \
         .floating-panel button:hover { background-color: #444444; border-color: #5a5a5a; } \
         .floating-panel button:active, .floating-panel button:checked { \
         color: #ffffff; background-color: #21699b; border-color: #15547f; } \
         .floating-panel button:checked label { color: #ffffff; font-weight: bold; } \
         .floating-panel .track-btn-mute:checked { background-color: #c0392b; border-color: #962d22; } \
         .floating-panel .track-btn-solo:checked { background-color: #d35400; border-color: #a04000; } \
         .floating-panel .track-btn-arm:checked { background-color: #c0392b; border-color: #962d22; } \
         .floating-panel .panel-close-button { color: #aaaaaa; background: transparent; \
         border-color: transparent; font-size: 18px; font-weight: 700; box-shadow: none; } \
         .floating-panel .panel-close-button:hover { color: #ffffff; background: #b43c45; } \
         .floating-panel entry, .floating-panel entry > text { color: #eeeeee; background-color: #222222; \
         border: 1px solid #444444; border-radius: 6px; caret-color: #eeeeee; box-shadow: none; padding: 4px 8px; } \
         .floating-panel entry:focus-within, .floating-panel entry > text:focus-within { \
         border-color: #21699b; background-color: #282828; } \
         .floating-panel scrolledwindow { background-color: #1e1e1e; border: 1px solid #383838; \
         border-radius: 6px; } \
         .floating-panel list { color: #eeeeee; background-color: transparent; } \
         .floating-panel row { color: #eeeeee; background-color: transparent; border-bottom: 1px solid #282828; \
         padding: 4px; border-radius: 4px; } \
         .floating-panel row:hover { background-color: #2a2a2a; } \
         .floating-panel row:selected { color: #ffffff; background-color: #1a4f78; } \
         .floating-panel row:selected label { color: #ffffff; font-weight: bold; }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );
}
