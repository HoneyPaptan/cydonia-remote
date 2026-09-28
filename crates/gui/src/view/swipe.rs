use crate::view::root::{Cydonia, ToggleChanges, narrow};
use bezel::gpui::{
    self, Context, DispatchPhase, IntoElement, ScrollWheelEvent, Styled, TouchPhase, Window,
};

const SWIPE_DISTANCE: f32 = 40.;
const FLICK_DISTANCE: f32 = 16.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Swipe {
    Following(f32),
    Spent,
}

impl Cydonia {
    pub(crate) fn swipe_listener(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let this = this.clone();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !narrow(window) {
                        return;
                    }
                    let taken = this
                        .update(cx, |this, cx| this.follow_swipe(event, window, cx))
                        .unwrap_or(false);
                    if taken {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    pub(crate) fn follow_swipe(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let delta = event.delta.pixel_delta(window.line_height());
        let across = f32::from(delta.x);
        match (event.touch_phase, self.swipe) {
            (TouchPhase::Started, _) => {
                self.swipe = (delta.x.abs() > delta.y.abs()).then_some(Swipe::Following(across));
            }
            (TouchPhase::Moved, Some(Swipe::Following(far))) => {
                self.swipe = Some(Swipe::Following(far + across));
            }
            (TouchPhase::Moved, Some(Swipe::Spent)) => {}
            (TouchPhase::Ended | TouchPhase::Cancelled, Some(swipe)) => {
                self.swipe = None;
                if let Swipe::Following(far) = swipe
                    && far.abs() >= FLICK_DISTANCE
                {
                    self.turn(far, window, cx);
                }
                return true;
            }
            _ => return false,
        }
        self.settle_swipe(window, cx);
        self.swipe.is_some()
    }

    fn settle_swipe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Swipe::Following(far)) = self.swipe else {
            return;
        };
        if far.abs() < SWIPE_DISTANCE {
            return;
        }
        self.swipe = Some(Swipe::Spent);
        self.turn(far, window, cx);
    }

    fn turn(&mut self, far: f32, window: &mut Window, cx: &mut Context<Self>) {
        match far > 0. {
            true => self.swipe_right(cx),
            false => self.swipe_left(window, cx),
        }
    }

    fn swipe_right(&mut self, cx: &mut Context<Self>) {
        if self.changes_open {
            self.hide_changes(cx);
        } else if !self.sidebar_open {
            self.toggle_sidebar(cx);
        }
    }

    fn swipe_left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_open {
            self.toggle_sidebar(cx);
        } else if !self.changes_open && self.shell_cwd(cx).is_some() {
            self.toggle_changes(&ToggleChanges, window, cx);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/swipe.rs"]
mod swipe_tests;
