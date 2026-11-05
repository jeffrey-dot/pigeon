//! Harp-aligned GTK tokens. Platform window controls remain native.
use gtk4::{self as gtk, prelude::*};

pub fn dark() -> bool {
    match std::env::var("PIGEON_APPEARANCE").as_deref() {
        Ok("dark") => true,
        Ok("light") => false,
        _ => gtk::Settings::default().is_some_and(|s| s.is_gtk_application_prefer_dark_theme()),
    }
}

pub fn install() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("theme.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn apply(window: &gtk::ApplicationWindow, dark: bool) {
    window.add_css_class("pigeon-window");
    if dark {
        window.add_css_class("dark");
    } else {
        window.remove_css_class("dark");
    }
}
