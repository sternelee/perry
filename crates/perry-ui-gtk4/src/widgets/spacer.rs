use gtk4::prelude::*;
use gtk4::Orientation;

/// Create a transparent spacer widget that expands to fill available space.
/// The expand direction is set when added to a parent (see `add_child`):
/// - In HStack: hexpand only
/// - In VStack: vexpand only
pub fn create() -> i64 {
    crate::app::ensure_gtk_init();
    let spacer = gtk4::Box::new(Orientation::Vertical, 0);
    spacer.add_css_class("perry-spacer");
    super::register_widget(spacer.upcast())
}
