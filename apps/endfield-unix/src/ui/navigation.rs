use gtk::prelude::*;

use super::pages;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    Optimize,
    Benchmark,
    Library,
    Diagnostics,
    Settings,
}

impl Page {
    pub const ALL: [Self; 6] = [
        Self::Home,
        Self::Optimize,
        Self::Benchmark,
        Self::Library,
        Self::Diagnostics,
        Self::Settings,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Optimize => "optimize",
            Self::Benchmark => "benchmark",
            Self::Library => "library",
            Self::Diagnostics => "diagnostics",
            Self::Settings => "settings",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Optimize => "Optimize",
            Self::Benchmark => "Benchmark",
            Self::Library => "Library",
            Self::Diagnostics => "Diagnostics",
            Self::Settings => "Settings",
        }
    }

    pub const fn icon_name(self) -> &'static str {
        match self {
            Self::Home => "go-home-symbolic",
            Self::Optimize => "applications-engineering-symbolic",
            Self::Benchmark => "utilities-system-monitor-symbolic",
            Self::Library => "folder-symbolic",
            Self::Diagnostics => "dialog-information-symbolic",
            Self::Settings => "preferences-system-symbolic",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.id() == id)
    }
}

pub struct NavigationLayout {
    pub split_view: adw::NavigationSplitView,
    pub page_stack: gtk::Stack,
}

pub fn build(on_navigate: impl Fn(Page) + Clone + 'static) -> NavigationLayout {
    let page_stack = gtk::Stack::new();
    page_stack.set_transition_type(gtk::StackTransitionType::None);
    page_stack.set_hexpand(true);
    page_stack.set_vexpand(true);

    for page in Page::ALL {
        let child = match page {
            Page::Home => pages::home::view(on_navigate.clone()),
            Page::Optimize => pages::optimize::view(),
            Page::Benchmark => pages::benchmark::view(),
            Page::Library => pages::library::view(),
            Page::Diagnostics => pages::diagnostics::view(),
            Page::Settings => pages::settings::view(),
        };
        let stack_page = page_stack.add_titled(&child, Some(page.id()), page.title());
        stack_page.set_icon_name(page.icon_name());
    }

    let stack_sidebar = gtk::StackSidebar::new();
    stack_sidebar.set_stack(&page_stack);
    stack_sidebar.set_vexpand(true);
    stack_sidebar.add_css_class("workspace-sidebar");

    let sidebar_content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    sidebar_content.set_margin_top(20);
    sidebar_content.set_margin_bottom(16);
    sidebar_content.set_margin_start(12);
    sidebar_content.set_margin_end(12);

    let section_label = gtk::Label::new(Some("WORKSPACE"));
    section_label.set_xalign(0.0);
    section_label.add_css_class("eyebrow");
    sidebar_content.append(&section_label);
    sidebar_content.append(&stack_sidebar);

    let sidebar_page = adw::NavigationPage::new(&sidebar_content, "Navigation");
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&page_stack));

    let content_page = adw::NavigationPage::new(&scroll, "Workspace");
    let split_view = adw::NavigationSplitView::new();
    split_view.set_sidebar(Some(&sidebar_page));
    split_view.set_content(Some(&content_page));
    split_view.set_min_sidebar_width(210.0);
    split_view.set_max_sidebar_width(280.0);
    split_view.set_sidebar_width_fraction(0.23);
    split_view.set_show_content(true);
    split_view.set_hexpand(true);
    split_view.set_vexpand(true);
    split_view.add_css_class("application-navigation");

    NavigationLayout {
        split_view,
        page_stack,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn page_id_title_and_icon_are_consistent_and_unique() {
        let ids: HashSet<_> = Page::ALL.into_iter().map(Page::id).collect();
        let titles: HashSet<_> = Page::ALL.into_iter().map(Page::title).collect();

        assert_eq!(Page::ALL.len(), 6);
        assert_eq!(ids.len(), 6);
        assert_eq!(titles.len(), 6);
        for page in Page::ALL {
            assert_eq!(Page::from_id(page.id()), Some(page));
            assert!(!page.icon_name().is_empty());
        }
        assert_eq!(Page::from_id("not-a-page"), None);
    }
}
