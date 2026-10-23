use super::navigation::Page;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMessage {
    Navigate(Page),
    BackToNavigation,
    NavigationLayoutChanged(bool),
    WindowCloseRequested,
    SmokeStart,
}
