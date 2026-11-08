use gtk::prelude::*;

use super::{card, flow_cards, page_content, section_title};
use crate::ui::navigation::Page;

pub(crate) fn view(navigate: impl Fn(Page) + Clone + 'static) -> gtk::Widget {
    let page = page_content(
        "DESKTOP SHELL",
        "Arknights: Endfield",
        "Review one explicitly selected installation, runtime pin, prefix, adapter, and executable candidate.",
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
        "Launch preflight reads the registered state and never starts Wine, Proton, UMU, or the game.",
    ));
    message.set_xalign(0.0);
    message.set_wrap(true);
    message.add_css_class("hero-copy");
    hero.append(&message);

    let selectors = gtk::Grid::new();
    selectors.set_row_spacing(8);
    selectors.set_column_spacing(10);
    let installation = gtk::Entry::new();
    installation.set_placeholder_text(Some("Registered game ID"));
    let prefix = gtk::Entry::new();
    prefix.set_placeholder_text(Some("Managed prefix ID"));
    let profile = gtk::Entry::new();
    profile.set_placeholder_text(Some("Profile (optional)"));
    let executable = gtk::Entry::new();
    executable.set_placeholder_text(Some("Selected executable, relative to game root"));
    let adapter = gtk::ComboBoxText::new();
    adapter.append_text("Managed Wine");
    adapter.append_text("External Wine");
    adapter.append_text("UMU-Proton");
    adapter.append_text("Direct Proton");
    adapter.set_active(Some(2));
    for (row, (name, widget)) in [
        (
            "Installation ID",
            installation.clone().upcast::<gtk::Widget>(),
        ),
        ("Prefix ID", prefix.clone().upcast::<gtk::Widget>()),
        ("Profile", profile.clone().upcast::<gtk::Widget>()),
        (
            "Executable candidate",
            executable.clone().upcast::<gtk::Widget>(),
        ),
        ("Adapter", adapter.clone().upcast::<gtk::Widget>()),
    ]
    .into_iter()
    .enumerate()
    {
        let label = gtk::Label::new(Some(name));
        label.set_xalign(0.0);
        selectors.attach(&label, 0, row as i32, 1, 1);
        selectors.attach(&widget, 1, row as i32, 1, 1);
    }
    hero.append(&selectors);

    let preflight_status = gtk::Label::new(Some("No launch plan has been prepared."));
    preflight_status.set_xalign(0.0);
    preflight_status.set_wrap(true);
    hero.append(&preflight_status);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.set_homogeneous(false);
    let play = gtk::Button::with_label("Play unavailable");
    play.set_sensitive(false);
    play.set_tooltip_text(Some(
        "Production execution is blocked until an approved prefix initializer and runtime invocation adapter are validated.",
    ));
    play.add_css_class("suggested-action");
    actions.append(&play);

    let check = gtk::Button::with_label("Prepare launch plan");
    let status = preflight_status.clone();
    let installation_entry = installation.clone();
    let prefix_entry = prefix.clone();
    let profile_entry = profile.clone();
    let executable_entry = executable.clone();
    let adapter_choice = adapter.clone();
    check.connect_clicked(move |_| {
        let installation_id = installation_entry.text().to_string();
        let prefix_id = prefix_entry.text().to_string();
        let profile_value = profile_entry.text().to_string();
        let profile_value = (!profile_value.is_empty()).then_some(profile_value);
        let executable_relative = executable_entry.text().to_string();
        let selected_adapter = match adapter_choice.active() {
            Some(0) => crate::cli::game_launch::AdapterArg::ManagedWine,
            Some(1) => crate::cli::game_launch::AdapterArg::ExternalWine,
            Some(2) => crate::cli::game_launch::AdapterArg::UmuProton,
            Some(3) => crate::cli::game_launch::AdapterArg::DirectProton,
            _ => {
                status.set_text("Select one supported runtime adapter.");
                return;
            }
        };
        status.set_text("Preparing a read-only launch plan…");
        let status_label = status.clone();
        gtk::glib::MainContext::default().spawn_local(async move {
            let result = gtk::gio::spawn_blocking(move || {
                crate::cli::game_launch::plan_for_ui(
                    installation_id,
                    prefix_id,
                    profile_value,
                    executable_relative,
                    selected_adapter,
                )
            })
            .await;
            match result {
                Ok(Ok(plan)) => {
                    let blockers = plan
                        .blockers
                        .iter()
                        .map(|blocker| blocker.code())
                        .collect::<Vec<_>>();
                    let warnings = plan
                        .warnings
                        .iter()
                        .map(|warning| format!("{warning:?}"))
                        .collect::<Vec<_>>();
                    status_label.set_text(&format!(
                        "Plan {} — {:?}. Installation: {} (root identity matches: {}). Runtime: {} / {} ({:?}, source {}, pin revision {:?}); adapter {:?}. Prefix: {} ({:?}, lifecycle {:?}, revision {:?}). Executable: {} (exists: {}, format {:?}). Approval required: {}. Process creation allowed: {}. Production execution: {}. Blockers: {}. Warnings: {}.",
                        plan.plan_id,
                        plan.state,
                        plan.installation_id,
                        plan.executable.root_identity_matches,
                        plan.runtime.runtime_id.as_deref().unwrap_or("unknown"),
                        plan.runtime.family.as_deref().unwrap_or("unknown"),
                        plan.runtime.state,
                        plan.runtime.selection_source.as_deref().unwrap_or("unknown"),
                        plan.runtime.pin_revision,
                        plan.runtime.adapter,
                        plan.prefix.prefix_id.as_deref().unwrap_or("unknown"),
                        plan.prefix.state,
                        plan.prefix.lifecycle,
                        plan.prefix.registry_revision,
                        plan.executable_relative,
                        plan.executable.exists,
                        plan.executable.format,
                        plan.approval_required,
                        plan.policy.process_creation_allowed,
                        if plan.process_execution_supported {
                            "available"
                        } else {
                            "unsupported"
                        },
                        if blockers.is_empty() {
                            "none".to_owned()
                        } else {
                            blockers.join(", ")
                        },
                        if warnings.is_empty() {
                            "none".to_owned()
                        } else {
                            warnings.join(", ")
                        },
                    ));
                }
                Ok(Err(error)) => status_label.set_text(&format!("Preflight unavailable: {error}")),
                Err(_) => status_label.set_text("Preflight worker failed before returning a plan."),
            }
        });
    });
    actions.append(&check);

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
            "Choose a P031 registration and one explicit installation-relative executable candidate.",
        ),
        -1,
    );
    statuses.insert(
        &card(
            "Runtime",
            "P027 pins are revalidated exactly. Stale profile pins do not fall back to machine pins.",
        ),
        -1,
    );
    statuses.insert(
        &card(
            "Optimization profile",
            "Created prefixes remain uninitialized; production Play is disabled while initialization is unsupported.",
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
