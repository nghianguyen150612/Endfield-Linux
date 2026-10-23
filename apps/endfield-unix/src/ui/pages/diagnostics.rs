use gtk::prelude::*;

use super::{card, flow_cards, page_content, section_title};

pub(crate) fn view() -> gtk::Widget {
    let page = page_content(
        "DEVELOPMENT STATUS",
        "Diagnostics",
        "This page reports application implementation status. It has not probed your system.",
    );

    page.append(&section_title("Available in this build"));
    let status = flow_cards();
    status.set_max_children_per_line(2);
    status.insert(
        &card(
            "Application shell",
            "GTK4 and libadwaita window with Relm4 navigation is available.",
        ),
        -1,
    );
    status.insert(
        &card(
            "System inspection",
            "GPU, kernel, driver, Vulkan, and runtime probes have not been implemented.",
        ),
        -1,
    );
    status.insert(
        &card(
            "Optimization planner",
            "The C17 ABI is present; P003 currently returns an empty plan set.",
        ),
        -1,
    );
    status.insert(
        &card(
            "Data collection",
            "No telemetry or benchmark data has been collected.",
        ),
        -1,
    );
    page.append(&status);
    page.upcast()
}
