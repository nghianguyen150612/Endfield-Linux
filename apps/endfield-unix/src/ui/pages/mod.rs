pub mod benchmark;
pub mod diagnostics;
pub mod home;
pub mod library;
pub mod optimize;
pub mod settings;

use gtk::prelude::*;

pub(super) fn page_content(eyebrow: &str, title: &str, description: &str) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 22);
    page.set_margin_top(30);
    page.set_margin_bottom(32);
    page.set_margin_start(32);
    page.set_margin_end(32);
    page.set_valign(gtk::Align::Start);
    page.add_css_class("page-content");

    let heading = gtk::Box::new(gtk::Orientation::Vertical, 7);
    heading.set_margin_bottom(3);

    let eyebrow_label = gtk::Label::new(Some(eyebrow));
    eyebrow_label.set_xalign(0.0);
    eyebrow_label.add_css_class("eyebrow");
    heading.append(&eyebrow_label);

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.add_css_class("page-heading");
    heading.append(&title_label);

    let description_label = gtk::Label::new(Some(description));
    description_label.set_xalign(0.0);
    description_label.set_wrap(true);
    description_label.add_css_class("page-description");
    heading.append(&description_label);

    page.append(&heading);
    page
}

pub(super) fn card(title: &str, detail: &str) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 9);
    card.set_hexpand(true);
    card.set_margin_bottom(2);
    card.set_margin_top(2);
    card.set_margin_start(2);
    card.set_margin_end(2);
    card.add_css_class("status-card");

    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.add_css_class("card-title");
    card.append(&title_label);

    let detail_label = gtk::Label::new(Some(detail));
    detail_label.set_xalign(0.0);
    detail_label.set_wrap(true);
    detail_label.set_valign(gtk::Align::Start);
    detail_label.add_css_class("card-detail");
    card.append(&detail_label);
    card
}

pub(super) fn section_title(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("section-heading");
    label
}

pub(super) fn info_row(title: &str, value: &str, detail: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.set_hexpand(true);
    row.add_css_class("info-row");

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
    labels.set_hexpand(true);
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.add_css_class("card-title");
    labels.append(&title_label);
    let detail_label = gtk::Label::new(Some(detail));
    detail_label.set_xalign(0.0);
    detail_label.set_wrap(true);
    detail_label.add_css_class("card-detail");
    labels.append(&detail_label);
    row.append(&labels);

    let value_label = gtk::Label::new(Some(value));
    value_label.set_xalign(1.0);
    value_label.set_valign(gtk::Align::Center);
    value_label.set_wrap(true);
    value_label.add_css_class("status-value");
    row.append(&value_label);
    row
}

pub(super) fn flow_cards() -> gtk::FlowBox {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(3);
    flow.set_row_spacing(10);
    flow.set_column_spacing(10);
    flow.set_homogeneous(true);
    flow.set_hexpand(true);
    flow.add_css_class("status-grid");
    flow
}
