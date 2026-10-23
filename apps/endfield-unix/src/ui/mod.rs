pub mod app;
pub mod messages;
pub mod model;
pub mod navigation;
pub mod pages;
pub mod resources;

use std::fmt;
use std::{cell::RefCell, fs, path::PathBuf, rc::Rc};

use relm4::RelmApp;

use crate::{APPLICATION_ID, APPLICATION_NAME, GuiStartupError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiError(String);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuiSmokeSummary {
    pub header_identity: String,
    pub navigation_destinations: usize,
    pub visited_pages: Vec<String>,
    pub page_titles: Vec<String>,
    pub initial_size: (i32, i32),
    pub resized_size: (i32, i32),
    pub window_resizable: bool,
    pub svg_loaded: bool,
    pub closed_cleanly: bool,
}

impl fmt::Display for GuiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GuiError {}

pub fn run() -> Result<(), GuiStartupError> {
    run_application(app::AppInit::default())
        .map_err(|error| GuiStartupError::new(error.to_string()))
}

fn initialize_graphics() -> Result<(), GuiError> {
    gtk::init().map_err(|error| {
        GuiError(format!(
            "could not initialize GTK display: {error}. Start endfield-unix from a graphical session."
        ))
    })?;
    adw::init().map_err(|error| {
        GuiError(format!(
            "could not initialize libadwaita: {error}. Start endfield-unix from a graphical session."
        ))
    })?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferDark);

    if !gtk::gio::Application::id_is_valid(APPLICATION_ID) {
        return Err(GuiError(format!(
            "invalid GTK application ID configured for {APPLICATION_NAME}: {APPLICATION_ID}"
        )));
    }

    relm4::set_global_css(resources::STYLE);
    Ok(())
}

fn run_application(init: app::AppInit) -> Result<(), GuiError> {
    initialize_graphics()?;
    RelmApp::<messages::AppMessage>::new(APPLICATION_ID)
        .with_args(vec![APPLICATION_NAME.to_owned()])
        .run::<app::App>(init);
    Ok(())
}

/// Runs a short in-process GTK smoke session for automated tests.
///
/// This is not exposed through the application command line. It opens the real
/// application window, walks all navigation destinations, resizes the window,
/// then closes it on the GTK main thread.
#[doc(hidden)]
pub fn run_smoke_test() -> Result<GuiSmokeSummary, GuiError> {
    let report = Rc::new(RefCell::new(GuiSmokeSummary::default()));
    let screenshot_path = std::env::var_os("ENDFIELD_GUI_SCREENSHOT").map(PathBuf::from);
    run_application(app::AppInit::with_smoke_report(
        report.clone(),
        screenshot_path.clone(),
    ))?;
    let summary = report.borrow().clone();

    if let Some(path) = screenshot_path {
        let screenshot = fs::metadata(&path).map_err(|error| {
            GuiError(format!(
                "GTK smoke test did not save the requested screenshot at {}: {error}",
                path.display()
            ))
        })?;
        if screenshot.len() == 0 {
            return Err(GuiError(format!(
                "GTK smoke test saved an empty screenshot at {}",
                path.display()
            )));
        }
    }

    let expected: Vec<_> = navigation::Page::ALL
        .into_iter()
        .map(|page| page.id().to_owned())
        .collect();
    if summary.header_identity != APPLICATION_NAME
        || summary.navigation_destinations != navigation::Page::ALL.len()
        || summary.visited_pages != expected
        || summary.page_titles
            != navigation::Page::ALL
                .into_iter()
                .map(|page| page.title().to_owned())
                .collect::<Vec<_>>()
        || summary.initial_size != (1100, 720)
        || summary.resized_size != (960, 660)
        || !summary.window_resizable
        || !summary.svg_loaded
        || !summary.closed_cleanly
    {
        return Err(GuiError(format!(
            "GTK smoke check did not meet expectations: {summary:?}"
        )));
    }

    Ok(summary)
}
