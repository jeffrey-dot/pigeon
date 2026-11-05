//! Small native widgets implementing docs/ui.md's single-line visual hierarchy.
use gtk4::{self as gtk, prelude::*};
use pigeon_core::api::StatusState as State;

pub fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class(class);
    label
}

pub fn icon_button(icon: &str, name: &str) -> gtk::Button {
    let button = named_button(name);
    button.set_icon_name(icon);
    button.add_css_class("pigeon-icon");
    button.set_tooltip_text(Some(name));
    button.set_focus_on_click(false);
    button
}

pub fn named_button(name: &str) -> gtk::Button {
    // GtkBuilder supports accessibility labels on the GTK 4.8 minimum version.
    let escaped = gtk::glib::markup_escape_text(name);
    let builder = gtk::Builder::from_string(&format!("<interface><object class=\"GtkButton\" id=\"button\"><accessibility><property name=\"label\">{escaped}</property></accessibility></object></interface>"));
    builder.object("button").unwrap_or_default()
}

pub fn status(state: Option<State>, unread: bool) -> gtk::Widget {
    if state == Some(State::Working) {
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(14, 14);
        spinner.set_halign(gtk::Align::Center);
        spinner.set_valign(gtk::Align::Center);
        spinner.add_css_class("pigeon-status");
        if gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations()) {
            spinner.start();
        }
        return spinner.upcast();
    }
    let icon = gtk::DrawingArea::new();
    icon.set_content_width(16);
    icon.set_content_height(16);
    icon.add_css_class("pigeon-status");
    if state == Some(State::Blocked) {
        icon.add_css_class("blocked");
    }
    if state == Some(State::Error) {
        icon.add_css_class("error");
    }
    if state == Some(State::Idle) || (state == Some(State::Done) && !unread) {
        icon.add_css_class("quiet");
    }
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    icon.set_draw_func(move |widget, context, _, _| {
        let color = widget.style_context().color();
        context.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        context.set_line_width(1.4);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        match state {
            Some(State::Blocked) => {
                context.arc(8.0, 8.0, 5.2, 0.0, std::f64::consts::TAU);
                let _ = context.stroke();
                context.arc(8.0, 8.0, 2.1, 0.0, std::f64::consts::TAU);
                let _ = context.fill();
            }
            Some(State::Done) => {
                if unread {
                    context.arc(8.0, 8.0, 6.0, 0.0, std::f64::consts::TAU);
                    let _ = context.fill();
                    let ink = if crate::theme::dark() { 0.14 } else { 1.0 };
                    context.set_source_rgb(ink, ink, ink);
                }
                context.move_to(4.7, 8.0);
                context.line_to(7.0, 10.2);
                context.line_to(11.1, 5.8);
                let _ = context.stroke();
            }
            Some(State::Idle) => {
                context.arc(8.3, 8.0, 5.2, 1.0, 5.3);
                context.arc_negative(11.0, 5.5, 5.0, 4.0, 1.9);
                let _ = context.fill();
            }
            Some(State::Error) => {
                context.move_to(8.0, 2.0);
                context.line_to(14.0, 13.0);
                context.line_to(2.0, 13.0);
                context.close_path();
                let _ = context.stroke();
                context.move_to(8.0, 6.0);
                context.line_to(8.0, 9.0);
                let _ = context.stroke();
                context.arc(8.0, 11.0, 0.7, 0.0, std::f64::consts::TAU);
                let _ = context.fill();
            }
            _ => {}
        }
    });
    icon.upcast()
}
