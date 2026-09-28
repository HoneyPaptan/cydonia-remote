use crate::view::root::{Cydonia, ToggleChanges, narrow};
use bezel::{
    gpui::{
        self, Context, DispatchPhase, IntoElement, LongPressEvent, Modifiers, MouseButton,
        MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Pixels, Point,
        ScrollWheelEvent, Styled, TouchDragEvent, TouchPhase, Window,
    },
    ui::touch,
};

const SWIPE_DISTANCE: f32 = 40.;
const FLICK_DISTANCE: f32 = 16.;
const EDGE: f32 = 24.;
const PULL_BAND: f32 = 64.;
const LIFT_DISTANCE: f32 = 8.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Swipe {
    Following(f32),
    Pulling(f32),
    Spent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Press {
    finger: Point<Pixels>,
    pointer: Point<Pixels>,
    dragging: bool,
}

impl Press {
    fn pointer_for(&self, finger: Point<Pixels>) -> Point<Pixels> {
        self.pointer + (finger - self.finger)
    }

    fn travelled(&self, finger: Point<Pixels>) -> bool {
        (finger - self.finger).magnitude() as f32 > LIFT_DISTANCE
    }
}

fn send(events: Vec<PlatformInput>, window: &Window, cx: &mut gpui::App) {
    window.defer(cx, move |window, cx| {
        for event in events {
            window.dispatch_event(event, cx);
        }
    });
}

fn press(button: MouseButton, position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    })
}

fn carry(position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    })
}

fn release(button: MouseButton, position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
    })
}

fn at_edge(position: Point<Pixels>, window: &Window) -> bool {
    let x = f32::from(position.x);
    x < EDGE || x > f32::from(window.viewport_size().width) - EDGE
}

impl Cydonia {
    pub(crate) fn swipe_listener(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let swiped = this.clone();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !narrow(window) {
                        return;
                    }
                    let taken = swiped
                        .update(cx, |this, cx| this.follow_swipe(event, window, cx))
                        .unwrap_or(false);
                    if taken {
                        cx.stop_propagation();
                    }
                });
                let held = this.clone();
                window.on_mouse_event(move |event: &LongPressEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        held.update(cx, |this, cx| this.follow_long_press(event, window, cx))
                            .ok();
                    }
                });
                let gripped = this.clone();
                window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        gripped
                            .update(cx, |this, cx| this.follow_grip(event, window, cx))
                            .ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn follow_long_press(&mut self, event: &LongPressEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.phase {
            TouchPhase::Started => {
                window.prevent_default();
                self.press = Some(Press {
                    finger: event.start_position,
                    pointer: event.start_position,
                    dragging: false,
                });
            }
            TouchPhase::Moved => {
                let Some(held) = self.press.as_mut() else {
                    return;
                };
                if held.dragging {
                    send(vec![carry(held.pointer_for(event.position))], window, cx);
                } else if held.travelled(event.position) {
                    held.dragging = true;
                    self.menu = None;
                    let start = held.pointer;
                    let to = held.pointer_for(event.position);
                    send(vec![press(MouseButton::Left, start), carry(to)], window, cx);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => match self.press.take() {
                Some(held) if held.dragging => {
                    let to = held.pointer_for(event.position);
                    send(vec![carry(to), release(MouseButton::Left, to)], window, cx);
                }
                Some(held) if event.phase == TouchPhase::Ended => {
                    let at = held.pointer;
                    send(
                        vec![press(MouseButton::Right, at), release(MouseButton::Right, at)],
                        window,
                        cx,
                    );
                }
                _ => {}
            },
        }
    }

    fn follow_grip(&mut self, event: &TouchDragEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.phase {
            TouchPhase::Started => {
                let Some(grip) = touch::grip_at(event.start_position) else {
                    return;
                };
                window.prevent_default();
                self.press = Some(Press {
                    finger: event.start_position,
                    pointer: grip,
                    dragging: true,
                });
                send(vec![press(MouseButton::Left, grip)], window, cx);
            }
            TouchPhase::Moved => {
                if let Some(held) = self.press.filter(|held| held.dragging) {
                    send(vec![carry(held.pointer_for(event.position))], window, cx);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if let Some(held) = self.press.take() {
                    let to = match held.travelled(event.position) {
                        true => held.pointer_for(event.position),
                        false => held.pointer,
                    };
                    send(vec![release(MouseButton::Left, to)], window, cx);
                }
            }
        }
    }

    pub(crate) fn follow_swipe(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let delta = event.delta.pixel_delta(window.line_height());
        let across = f32::from(delta.x);
        let down = f32::from(delta.y);
        match (event.touch_phase, self.swipe) {
            (TouchPhase::Started, _) => {
                self.swipe = self.start_swipe(event.position, across, down, window);
            }
            (TouchPhase::Moved, Some(Swipe::Following(far))) => {
                self.swipe = Some(Swipe::Following(far + across));
            }
            (TouchPhase::Moved, Some(Swipe::Pulling(far))) => {
                self.swipe = Some(Swipe::Pulling(far + down));
            }
            (TouchPhase::Moved, Some(Swipe::Spent)) => {}
            (TouchPhase::Ended | TouchPhase::Cancelled, Some(swipe)) => {
                self.swipe = None;
                match swipe {
                    Swipe::Following(far) if far.abs() >= FLICK_DISTANCE => {
                        self.turn(far, window, cx)
                    }
                    Swipe::Pulling(far) if far.abs() >= FLICK_DISTANCE => self.pull(far, cx),
                    _ => {}
                }
                return true;
            }
            _ => return false,
        }
        self.settle_swipe(window, cx);
        self.swipe.is_some()
    }

    fn start_swipe(&self, at: Point<Pixels>, across: f32, down: f32, window: &Window) -> Option<Swipe> {
        if across.abs() > down.abs() {
            let owned = !at_edge(at, window) && touch::scrolls_sideways_at(at);
            return (!owned && !self.quick).then_some(Swipe::Following(across));
        }
        let from_top = f32::from(at.y) < PULL_BAND && down > 0.;
        (from_top || (self.quick && down < 0.)).then_some(Swipe::Pulling(down))
    }

    fn settle_swipe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.swipe {
            Some(Swipe::Following(far)) if far.abs() >= SWIPE_DISTANCE => {
                self.swipe = Some(Swipe::Spent);
                self.turn(far, window, cx);
            }
            Some(Swipe::Pulling(far)) if far.abs() >= SWIPE_DISTANCE => {
                self.swipe = Some(Swipe::Spent);
                self.pull(far, cx);
            }
            _ => {}
        }
    }

    fn pull(&mut self, far: f32, cx: &mut Context<Self>) {
        match far > 0. {
            true => self.open_quick_actions(cx),
            false => {
                self.close_quick_actions(cx);
            }
        }
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
