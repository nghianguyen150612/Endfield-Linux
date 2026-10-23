use gtk::prelude::*;

use super::{card, page_content, section_title};

pub(crate) fn view() -> gtk::Widget {
    let page = page_content(
        "APPLICATION",
        "Settings",
        "Preference controls will be added with the versioned configuration system. Nothing on this page is persisted.",
    );

    page.append(&section_title("General"));
    page.append(&card(
        "Application preferences",
        "Language, launch behavior, and appearance preferences are not configurable yet.",
    ));

    page.append(&section_title("Privacy"));
    page.append(&card(
        "Local by default",
        "This frontend shell does not collect telemetry, inspect private files, contact game services, or start a background daemon.",
    ));

    page.append(&section_title("About"));
    page.append(&card(
        "endfield-unix",
        "Independent community software for Arknights: Endfield. GTK interface support is currently Linux-focused; macOS and BSD behavior is unverified.",
    ));
    page.upcast()
}
