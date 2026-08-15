//! Application stylesheet for the status bar and floating panels.

use gtk4 as gtk;

pub fn apply_stylesheet() {
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_data(
        ".status-bar { font-family: monospace; font-size: 14px; font-weight: bold; \
         padding: 6px 10px; background: #1a1a1a; color: #eee; \
         min-height: 22px; } \
         .floating-panel { padding: 12px; color: #18202a; \
         background: rgba(248, 250, 252, 0.98); border: 1px solid #8795a6; \
         border-radius: 10px; box-shadow: 0 7px 22px rgba(0, 0, 0, 0.42); } \
         .floating-panel label { color: #18202a; } \
         .floating-panel .panel-title { color: #101820; font-size: 16px; font-weight: 700; } \
         .floating-panel .panel-hint { color: #425466; font-size: 13px; } \
         .floating-panel button { color: #17202a; background: #e6ecf3; \
         border: 1px solid #a5b1bf; border-radius: 6px; } \
         .floating-panel button:hover { background: #d5e4f3; border-color: #5685ad; } \
         .floating-panel button:active, .floating-panel button:checked { \
         color: #ffffff; background: #21699b; border-color: #15547f; } \
         .floating-panel .panel-close-button { color: #334155; background: transparent; \
         border-color: transparent; font-size: 18px; font-weight: 700; } \
         .floating-panel .panel-close-button:hover { color: #ffffff; background: #b43c45; } \
         .floating-panel entry { color: #111827; background: #ffffff; \
         border: 1px solid #93a1b2; caret-color: #111827; } \
         .floating-panel list { color: #18202a; background: #ffffff; \
         border: 1px solid #a5b1bf; } \
         .floating-panel row { color: #18202a; background: #ffffff; } \
         .floating-panel row:hover { background: #e4eef7; } \
         .floating-panel row:selected { color: #ffffff; background: #21699b; } \
         .floating-panel row:selected label { color: #ffffff; }",
    );
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &css_provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
