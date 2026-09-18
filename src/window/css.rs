//! Application stylesheet for the status bar and floating panels.

use gtk4 as gtk;

pub fn apply_stylesheet() {
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_data(
        ".status-bar { font-family: monospace; font-size: 14px; font-weight: bold; \
         padding: 6px 10px; background: #1a1a1a; color: #eee; \
         min-height: 22px; } \
         .floating-panel { padding: 12px; color: #eeeeee; \
         background: rgba(26, 26, 26, 0.95); border: 1px solid #333333; \
         border-radius: 10px; box-shadow: 0 7px 22px rgba(0, 0, 0, 0.7); } \
         .floating-panel label { color: #eeeeee; } \
         .floating-panel .panel-title { color: #ffffff; font-size: 16px; font-weight: 700; } \
         .floating-panel .panel-hint { color: #a0a0a0; font-size: 13px; } \
         .floating-panel button { color: #eeeeee; background: #333333; \
         border: 1px solid #444444; border-radius: 6px; } \
         .floating-panel button:hover { background: #444444; border-color: #555555; } \
         .floating-panel button:active, .floating-panel button:checked { \
         color: #ffffff; background: #21699b; border-color: #15547f; } \
         .floating-panel .panel-close-button { color: #a0a0a0; background: transparent; \
         border-color: transparent; font-size: 18px; font-weight: 700; } \
         .floating-panel .panel-close-button:hover { color: #ffffff; background: #b43c45; } \
         .floating-panel entry { color: #eeeeee; background: #222222; \
         border: 1px solid #444444; caret-color: #eeeeee; } \
         .floating-panel list { color: #eeeeee; background: #222222; \
         border: 1px solid #333333; } \
         .floating-panel row { color: #eeeeee; background: #222222; } \
         .floating-panel row:hover { background: #333333; } \
         .floating-panel row:selected { color: #ffffff; background: #21699b; } \
         .floating-panel row:selected label { color: #ffffff; }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
