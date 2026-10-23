use gtk::prelude::*;

use super::{card, flow_cards, info_row, page_content, section_title};

pub(crate) fn view() -> gtk::Widget {
    let page = page_content(
        "GOALS AND PLANS",
        "Optimize",
        "Set the direction for future recommendations. This shell does not apply these goals or generate optimization plans.",
    );

    page.append(&section_title("Performance goals"));
    let goals = gtk::Box::new(gtk::Orientation::Vertical, 0);
    goals.add_css_class("status-card");
    goals.append(&info_row(
        "Target FPS",
        "Not set",
        "A target can be set when the planner UI is implemented.",
    ));
    goals.append(&info_row(
        "Playing experience",
        "Not set",
        "Smoothness, visual quality, and latency preferences are not configured.",
    ));
    goals.append(&info_row(
        "Optimization intensity",
        "L0–L4",
        "The intensity scale is defined; no level is active in this shell.",
    ));
    page.append(&goals);

    page.append(&section_title("System capabilities"));
    let capabilities = flow_cards();
    capabilities.set_max_children_per_line(2);
    capabilities.insert(
        &card(
            "Hardware snapshot",
            "Unavailable. Hardware and driver detection has not been implemented.",
        ),
        -1,
    );
    capabilities.insert(
        &card(
            "Runtime information",
            "Unavailable. No runtime probe is active.",
        ),
        -1,
    );
    page.append(&capabilities);

    page.append(&section_title("Optimization candidates"));
    page.append(&card(
        "No optimization plans available yet.",
        "The P003 C17 ABI validates requests and returns an empty plan set. Candidate generation and recommendations are not implemented.",
    ));
    page.append(&card(
        "Review and approval",
        "Future plans will show their evidence, uncertainty, trade-offs, and rollback requirements before asking for approval.",
    ));

    page.append(&section_title("Benchmark results"));
    let results = flow_cards();
    results.set_max_children_per_line(3);
    results.insert(&card("Baseline", "Unavailable"), -1);
    results.insert(&card("Results", "Unavailable"), -1);
    results.insert(&card("Comparison and rollback", "Unavailable"), -1);
    page.append(&results);

    page.upcast()
}
