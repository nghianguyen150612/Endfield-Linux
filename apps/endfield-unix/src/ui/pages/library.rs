use gtk::prelude::*;

use super::{card, page_content, section_title};

pub(crate) fn view() -> gtk::Widget {
    let page = page_content(
        "GAME MANAGEMENT",
        "Library",
        "Game installation and import are not configured in this version.",
    );

    page.append(&section_title("Your games"));
    page.append(&card(
        "No games configured",
        "The library is empty because game installation and import workflows have not been implemented. No files were scanned.",
    ));

    let import = gtk::Button::with_label("Import a game — unavailable");
    import.set_sensitive(false);
    import.set_tooltip_text(Some(
        "Game import becomes available when a verified library workflow is implemented.",
    ));
    import.set_halign(gtk::Align::Start);
    page.append(&import);
    page.upcast()
}
