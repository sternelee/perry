use gtk4::prelude::*;
use gtk4::Orientation;

/// Create a GtkBox with vertical orientation.
pub fn create(spacing: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let vbox = gtk4::Box::new(Orientation::Vertical, spacing as i32);
    super::register_widget(vbox.upcast())
}

/// Create a GtkBox with vertical orientation and custom edge insets.
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let vbox = gtk4::Box::new(Orientation::Vertical, spacing as i32);
    vbox.set_margin_top(top as i32);
    vbox.set_margin_bottom(bottom as i32);
    vbox.set_margin_start(left as i32);
    vbox.set_margin_end(right as i32);
    super::register_widget(vbox.upcast())
}
