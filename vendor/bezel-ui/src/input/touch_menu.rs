use super::*;
use crate::popover::{self, menu_row};

const MENU_GAP: f32 = 8.;

impl TextField {
    pub(super) fn open_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let offset = self.index_for_mouse_position(event.position, self.line_height());
        if !self.selected_range.contains(&offset) {
            self.select_word_at(offset, cx);
        }
        self.touch_menu = Some(event.position);
        cx.notify();
    }

    fn select_word_at(&mut self, offset: usize, cx: &mut Context<Self>) {
        let word = word_around(&self.content, offset);
        self.move_to(word.start, cx);
        self.select_to(word.end, cx);
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.touch_menu = None;
        cx.notify();
    }

    fn menu_entry(
        &self,
        label: &'static str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui::AnyElement {
        let theme = Theme::of(cx);
        menu_row(&theme, false, None)
            .id(label)
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    run(this, window, cx);
                    this.close_menu(cx);
                }),
            )
            .into_any_element()
    }

    pub(super) fn menu_layer(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let at = self.touch_menu?;
        if !self.focus_handle.is_focused(window) {
            self.touch_menu = None;
            return None;
        }
        let selected = !self.selected_range.is_empty();
        let pasteable = cx.read_from_clipboard().is_some_and(|item| item.text().is_some());
        let mut entries = Vec::new();
        if self.content.len() > self.selected_range.len() {
            entries.push(self.menu_entry("Select all", cx, |this, window, cx| {
                this.select_all(&SelectAll, window, cx)
            }));
        }
        if selected {
            entries.push(self.menu_entry("Cut", cx, |this, window, cx| {
                this.cut(&Cut, window, cx)
            }));
            entries.push(self.menu_entry("Copy", cx, |this, window, cx| {
                this.copy(&Copy, window, cx)
            }));
        }
        if pasteable {
            entries.push(self.menu_entry("Paste", cx, |this, window, cx| {
                this.paste(&Paste, window, cx)
            }));
        }
        if entries.is_empty() {
            self.touch_menu = None;
            return None;
        }
        let bar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(2.))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_menu(cx)))
            .children(entries);
        Some(popover::menu_above_at(
            format!("text-field-menu-{}", cx.entity_id().as_u64()),
            gpui::point(at.x, at.y - px(MENU_GAP)),
            bar.into_any_element(),
            None,
        ))
    }
}
