use gtk::prelude::*;

use super::{card, flow_cards, page_content, section_title};
use crate::ui::navigation::Page;

pub(crate) fn view(navigate: impl Fn(Page) + Clone + 'static) -> gtk::Widget {
    let page = page_content(
        "DESKTOP SHELL",
        "Arknights: Endfield",
        "A community launcher workspace for Linux. Game setup, runtime management, and performance services are not implemented yet.",
    );

    let hero = gtk::Box::new(gtk::Orientation::Vertical, 14);
    hero.set_margin_top(2);
    hero.set_margin_bottom(2);
    hero.set_margin_start(2);
    hero.set_margin_end(2);
    hero.add_css_class("hero-panel");

    let state_label = gtk::Label::new(Some("NOT CONFIGURED"));
    state_label.set_xalign(0.0);
    state_label.add_css_class("state-label");
    hero.append(&state_label);

    let message = gtk::Label::new(Some(
        "Configure a game installation and runtime when those services become available.",
    ));
    message.set_xalign(0.0);
    message.set_wrap(true);
    message.add_css_class("hero-copy");
    hero.append(&message);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.set_homogeneous(false);
    let play = gtk::Button::with_label("Play unavailable");
    play.set_sensitive(false);
    play.set_tooltip_text(Some(
        "Play is unavailable until a game launch backend is implemented.",
    ));
    play.add_css_class("suggested-action");
    actions.append(&play);

    let optimize = gtk::Button::with_label("Review optimization goals");
    let navigate_optimize = navigate.clone();
    optimize.connect_clicked(move |_| navigate_optimize(Page::Optimize));
    actions.append(&optimize);

    let library = gtk::Button::with_label("Open library");
    library.connect_clicked(move |_| navigate(Page::Library));
    actions.append(&library);
    hero.append(&actions);
    page.append(&hero);

    page.append(&section_title("Current status"));
    let statuses = flow_cards();
    statuses.insert(
        &card(
            "Game installation",
            "Not configured. Import and installation support are not available yet.",
        ),
        -1,
    );
    statuses.insert(
        &card(
            "Runtime",
        "Runtime setup is not implemented yet. Runtime availability is unknown; no probe has run.",
        ),
        -1,
    );
    statuses.insert(
        &card(
            "Optimization profile",
            "No profile selected. Optimization data is unavailable.",
        ),
        -1,
    );
    page.append(&statuses);

    let note = gtk::Label::new(Some(
        "Independent community software. Not affiliated with the game publisher.",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("quiet-note");
    page.append(&note);

    page.upcast()
}
