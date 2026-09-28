//! What the app draws. Every view here reads
//! [`crate::model::workspace::Workspace`] and writes to it by name; none of
//! them owns app state.

pub mod arrangement;
pub mod back;
pub mod article;
pub mod board;
pub mod chrome;
pub mod component;
pub mod confirm;
pub mod create;
pub mod desktop;
pub mod folders;
pub mod detail;
pub mod find;
pub mod header;
#[cfg(feature = "desktop")]
pub mod hotkey;
pub mod info;
pub mod keymap;
pub mod leaf;
pub mod menubar;
pub mod root;
pub mod picture;
pub mod quick;
pub mod search;
pub mod section;
pub mod settings;
pub mod sidebar;
pub mod swipe;
pub mod table;

pub(crate) fn focus_for_typing(
    handle: &bezel::gpui::FocusHandle,
    window: &mut bezel::gpui::Window,
    cx: &mut bezel::gpui::App,
) {
    window.focus(handle, cx);
    window.request_virtual_keyboard();
}

#[cfg(test)]
#[path = "../../tests/unit/clipboard.rs"]
pub(crate) mod clipboard_tests;
