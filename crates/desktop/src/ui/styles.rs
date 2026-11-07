use super::*;

pub(super) fn install() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("style.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub(super) fn disclosure(expander: &gtk::Expander, icon: &str) {
    let title = expander.label().unwrap_or_default();
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("disclosure-icon");
    header.append(&image);
    let label = gtk::Label::builder()
        .label(title.as_str())
        .xalign(0.0)
        .hexpand(true)
        .build();
    label.add_css_class("heading");
    header.append(&label);
    expander.set_label_widget(Some(&header));
    expander.add_css_class("disclosure");
    expander.connect_expanded_notify(|expander| {
        if expander.is_expanded() {
            expander.add_css_class("is-open");
        } else {
            expander.remove_css_class("is-open");
        }
    });
}
