use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use adw::prelude::*;
use gtk::glib;
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::{APPLICATION_NAME, ui::resources};

use super::{
    GuiSmokeSummary,
    messages::AppMessage,
    model::{AppModel, WindowLifecycle},
    navigation::{self, Page},
};

#[derive(Default)]
pub struct AppInit {
    smoke_report: Option<Rc<RefCell<GuiSmokeSummary>>>,
}

impl AppInit {
    pub(super) fn with_smoke_report(report: Rc<RefCell<GuiSmokeSummary>>) -> Self {
        Self {
            smoke_report: Some(report),
        }
    }
}

pub struct AppWidgets {
    navigation: navigation::NavigationLayout,
    page_title: gtk::Label,
    back_button: gtk::Button,
}

#[derive(Debug)]
pub struct App {
    state: AppModel,
    smoke_report: Option<Rc<RefCell<GuiSmokeSummary>>>,
    smoke_step: usize,
    smoke_selection_pending: Option<Rc<Cell<bool>>>,
    smoke_close_pending: Option<Rc<Cell<bool>>>,
}

impl SimpleComponent for App {
    type Input = AppMessage;
    type Output = ();
    type Init = AppInit;
    type Root = adw::ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::new(&relm4::main_application())
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let state = AppModel {
            lifecycle: WindowLifecycle::Running,
            ..AppModel::default()
        };

        root.set_title(Some(APPLICATION_NAME));
        root.set_default_size(1100, 720);
        root.set_resizable(true);

        let header_bar = adw::HeaderBar::new();
        header_bar.add_css_class("application-header");

        let back_button = gtk::Button::with_label("Back");
        back_button.set_tooltip_text(Some("Return to navigation"));
        back_button.set_visible(false);
        back_button.add_css_class("flat");
        let back_sender = sender.clone();
        back_button.connect_clicked(move |_| {
            back_sender.input(AppMessage::BackToNavigation);
        });
        header_bar.pack_start(&back_button);

        let title_area = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        title_area.set_valign(gtk::Align::Center);
        let (brand_image, svg_loaded) = resources::brand_image();
        title_area.append(&brand_image);

        let app_name = gtk::Label::new(Some(APPLICATION_NAME));
        app_name.add_css_class("application-identity");
        title_area.append(&app_name);

        let divider = gtk::Separator::new(gtk::Orientation::Vertical);
        title_area.append(&divider);

        let page_title = gtk::Label::new(Some(Page::Home.title()));
        page_title.add_css_class("page-title-in-header");
        title_area.append(&page_title);
        header_bar.set_title_widget(Some(&title_area));

        let settings_button = gtk::Button::with_label("Settings");
        settings_button.set_tooltip_text(Some("Open application settings"));
        settings_button.add_css_class("flat");
        let settings_sender = sender.clone();
        settings_button.connect_clicked(move |_| {
            settings_sender.input(AppMessage::Navigate(Page::Settings));
        });
        header_bar.pack_end(&settings_button);

        let navigation_sender = sender.clone();
        let navigation = navigation::build(move |page| {
            navigation_sender.input(AppMessage::Navigate(page));
        });

        let layout_sender = sender.clone();
        navigation
            .split_view
            .connect_collapsed_notify(move |split_view| {
                layout_sender.input(AppMessage::NavigationLayoutChanged(
                    split_view.is_collapsed(),
                ));
            });

        let selection_sender = sender.clone();
        navigation
            .page_stack
            .connect_visible_child_name_notify(move |stack| {
                if let Some(name) = stack.visible_child_name() {
                    if let Some(page) = Page::from_id(name.as_str()) {
                        selection_sender.input(AppMessage::Navigate(page));
                    }
                }
            });

        let close_sender = sender.clone();
        let close_report = init.smoke_report.clone();
        root.connect_close_request(move |_| {
            close_sender.input(AppMessage::WindowCloseRequested);
            if let Some(report) = &close_report {
                report.borrow_mut().closed_cleanly = true;
            }
            glib::Propagation::Proceed
        });

        root.set_titlebar(Some(&header_bar));
        root.set_content(Some(&navigation.split_view));

        let widgets = AppWidgets {
            navigation,
            page_title,
            back_button,
        };

        if let Some(report) = &init.smoke_report {
            let mut report = report.borrow_mut();
            report.header_identity = app_name.label().to_string();
            report.navigation_destinations =
                widgets.navigation.page_stack.pages().n_items() as usize;
            report.initial_size = root.default_size();
            report.window_resizable = root.is_resizable();
            report.svg_loaded = svg_loaded;
            report.visited_pages.push(Page::Home.id().to_owned());
            report
                .page_titles
                .push(widgets.page_title.label().to_string());

            let smoke_sender = sender.clone();
            glib::idle_add_local_once(move || smoke_sender.input(AppMessage::SmokeStart));
        }

        let smoke_report = init.smoke_report;
        let model = App {
            state,
            smoke_step: usize::from(smoke_report.is_some()),
            smoke_selection_pending: smoke_report.as_ref().map(|_| Rc::new(Cell::new(false))),
            smoke_close_pending: smoke_report.as_ref().map(|_| Rc::new(Cell::new(false))),
            smoke_report,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            AppMessage::Navigate(page) => {
                self.state.navigation.navigate_to(page);
                if self.smoke_report.is_some() && Page::ALL.get(self.smoke_step) == Some(&page) {
                    self.smoke_step += 1;
                }
            }
            AppMessage::BackToNavigation => self.state.navigation.show_navigation(),
            AppMessage::NavigationLayoutChanged(compact) => {
                self.state.navigation.set_compact(compact)
            }
            AppMessage::WindowCloseRequested => self.state.lifecycle = WindowLifecycle::Closing,
            AppMessage::SmokeStart => {}
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        let page = self.state.navigation.active_page;
        widgets
            .navigation
            .page_stack
            .set_visible_child_name(page.id());
        widgets.page_title.set_label(page.title());
        widgets
            .back_button
            .set_visible(self.state.navigation.show_back_button());
        widgets
            .navigation
            .split_view
            .set_show_content(self.state.navigation.content_revealed);

        if let Some(report) = &self.smoke_report {
            let visible = widgets.navigation.page_stack.visible_child_name();
            if visible.is_some_and(|name| name.as_str() == page.id()) {
                let mut report = report.borrow_mut();
                if report.visited_pages.last().map(String::as_str) != Some(page.id()) {
                    report.visited_pages.push(page.id().to_owned());
                    report
                        .page_titles
                        .push(widgets.page_title.label().to_string());
                }
            }

            if self.smoke_step < Page::ALL.len() {
                if let Some(pending) = &self.smoke_selection_pending {
                    if !pending.replace(true) {
                        let pending = pending.clone();
                        let stack = widgets.navigation.page_stack.clone();
                        if let Some(next_page) = Page::ALL.get(self.smoke_step).copied() {
                            glib::idle_add_local_once(move || {
                                pending.set(false);
                                stack.set_visible_child_name(next_page.id());
                            });
                        }
                    }
                }
            } else if let Some(pending) = &self.smoke_close_pending {
                if pending.replace(true) {
                    return;
                }
                let report = self.smoke_report.clone();
                glib::timeout_add_local_once(Duration::from_millis(120), move || {
                    let app = relm4::main_application();
                    if let Some(window) = app.active_window() {
                        window.set_default_size(960, 660);
                        if let Some(report) = &report {
                            report.borrow_mut().resized_size = window.default_size();
                        }
                        window.close();
                    } else {
                        app.quit();
                    }
                });
            }
        }
    }
}
