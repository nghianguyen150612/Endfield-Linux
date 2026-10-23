use gtk::prelude::*;

use super::{card, page_content, section_title};

pub(crate) fn view() -> gtk::Widget {
    let page = page_content(
        "MEASUREMENTS",
        "Benchmark",
        "Performance measurement is not available yet. No FPS or frametime data is collected by this application shell.",
    );

    page.append(&section_title("Performance history"));
    let graph_area = gtk::Box::new(gtk::Orientation::Vertical, 10);
    graph_area.set_hexpand(true);
    graph_area.set_vexpand(true);
    graph_area.set_size_request(-1, 230);
    graph_area.set_valign(gtk::Align::Start);
    graph_area.add_css_class("chart-placeholder");

    let title = gtk::Label::new(Some("No benchmark data"));
    title.add_css_class("card-title");
    graph_area.append(&title);
    let detail = gtk::Label::new(Some(
        "FPS and frametime graphs will appear here after a measurement service is implemented.",
    ));
    detail.set_wrap(true);
    detail.set_justify(gtk::Justification::Center);
    detail.add_css_class("card-detail");
    graph_area.append(&detail);
    page.append(&graph_area);

    page.append(&section_title("Measurement details"));
    page.append(&card(
        "No results recorded",
        "There is no baseline, benchmark run, or comparison to show. No performance change is estimated.",
    ));
    page.upcast()
}
