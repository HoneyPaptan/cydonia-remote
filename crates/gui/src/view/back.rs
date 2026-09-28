//! The phone's back gesture: it steps out of whatever is over the screen, then
//! out of what is open, and reports when it is already on the launch view so
//! the app itself can go.

use crate::view::{root::Cydonia, search::DismissSearch};
use bezel::gpui::{Context, Window};

impl Cydonia {
    /// Undo the last step in, and whether there was one to undo.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let stepped = self.close_overlay(window, cx) || self.close_column(window, cx);
        if stepped {
            cx.notify();
            return true;
        }
        if self.leaf().entry.is_some() {
            self.close_focused_pane(window, cx);
            return true;
        }
        self.put_away_front(cx)
    }

    /// Back to the launch view from whatever the project has in front, which
    /// on a phone is the project's own pick rather than a tab to close.
    fn put_away_front(&mut self, cx: &mut Context<Self>) -> bool {
        self.workspace.update(cx, |workspace, cx| {
            let Some(open) = workspace
                .active
                .and_then(|project| workspace.projects.get_mut(project))
            else {
                return false;
            };
            let put_away = [
                open.active.take().is_some(),
                open.board.take().is_some(),
                open.article.take().is_some(),
                open.table.take().is_some(),
            ]
            .contains(&true);
            cx.notify();
            put_away
        })
    }

    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        #[cfg(not(feature = "desktop"))]
        if let Some(sheet) = self.settings_sheet.clone() {
            if !sheet.update(cx, |sheet, cx| sheet.back(cx)) {
                self.settings_sheet = None;
            }
            return true;
        }
        if self.quick {
            return self.close_quick_actions(cx);
        }
        if self.search.open {
            self.dismiss_search(&DismissSearch, window, cx);
            return true;
        }
        [
            self.menu.take().is_some(),
            self.confirming.take().is_some(),
            self.making.take().is_some(),
            self.desktop_only.take().is_some(),
            self.info.take().is_some(),
            self.renaming.take().is_some(),
        ]
        .contains(&true)
    }

    fn close_column(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        if self.sidebar_open && !self.sidebar_docked(window) {
            self.sidebar_open = false;
            return true;
        }
        if !self.changes_open {
            return false;
        }
        let tree_closed = self
            .changes
            .clone()
            .is_some_and(|panel| panel.update(cx, |panel, cx| panel.close_files(cx)));
        if !tree_closed {
            self.hide_changes(cx);
        }
        true
    }
}
