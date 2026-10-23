use super::navigation::Page;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowLifecycle {
    Starting,
    Running,
    Closing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationState {
    pub active_page: Page,
    pub compact: bool,
    pub content_revealed: bool,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            active_page: Page::Home,
            compact: false,
            content_revealed: true,
        }
    }
}

impl NavigationState {
    pub fn navigate_to(&mut self, page: Page) {
        self.active_page = page;
        if self.compact {
            self.content_revealed = true;
        }
    }

    pub fn set_compact(&mut self, compact: bool) {
        self.compact = compact;
        if !compact {
            self.content_revealed = true;
        }
    }

    pub fn show_navigation(&mut self) {
        if self.compact {
            self.content_revealed = false;
        }
    }

    pub const fn show_back_button(self) -> bool {
        self.compact && self.content_revealed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppModel {
    pub navigation: NavigationState,
    pub lifecycle: WindowLifecycle,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            navigation: NavigationState::default(),
            lifecycle: WindowLifecycle::Starting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_navigation_selects_home() {
        assert_eq!(NavigationState::default().active_page, Page::Home);
    }

    #[test]
    fn navigation_transitions_update_page_and_compact_presentation() {
        let mut state = NavigationState::default();
        state.set_compact(true);
        state.navigate_to(Page::Library);

        assert_eq!(state.active_page, Page::Library);
        assert!(state.content_revealed);
        assert!(state.show_back_button());

        state.show_navigation();
        assert!(!state.content_revealed);
        assert!(!state.show_back_button());
    }

    #[test]
    fn expanding_window_restores_persistent_navigation() {
        let mut state = NavigationState::default();
        state.set_compact(true);
        state.show_navigation();
        state.set_compact(false);

        assert!(state.content_revealed);
        assert!(!state.show_back_button());
    }
}
