use std::cell::RefCell;

use gpui::{App, Bounds, IntoElement, Pixels, Point, ScrollHandle, Styled, Window, canvas, px, size};

use crate::cover::{self, Mark};

const GRIP_REACH: f32 = 12.;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Sideways,
    Grip,
    Pull,
    Scroll,
}

thread_local! {
    static REGIONS: RefCell<Vec<(Kind, Mark, Bounds<Pixels>)>> = const { RefCell::new(Vec::new()) };
}

pub fn forget() {
    REGIONS.with_borrow_mut(Vec::clear);
}

pub fn sideways() -> impl IntoElement {
    mark(Kind::Sideways).inset_0()
}

pub fn sideways_when_overflowing(handle: &ScrollHandle) -> impl IntoElement {
    let handle = handle.clone();
    marked_when(Kind::Sideways, move || handle.max_offset().x > px(0.)).inset_0()
}

pub fn scrolls() -> impl IntoElement {
    mark(Kind::Scroll).inset_0()
}

pub fn grip() -> impl IntoElement {
    let reach = px(-GRIP_REACH);
    mark(Kind::Grip).top(reach).left(reach).right(reach).bottom(reach)
}

pub fn mark_pull(bounds: Bounds<Pixels>) {
    record(Kind::Pull, bounds);
}

pub fn mark_scroll(bounds: Bounds<Pixels>) {
    record(Kind::Scroll, bounds);
}

pub fn pulls_at(position: Point<Pixels>, window: &Window, cx: &App) -> bool {
    found(Kind::Pull, position, window, cx).is_some()
}

pub fn scrolls_at(position: Point<Pixels>, window: &Window, cx: &App) -> bool {
    found(Kind::Scroll, position, window, cx).is_some()
}

pub fn scrolls_sideways_at(position: Point<Pixels>, window: &Window, cx: &App) -> bool {
    found(Kind::Sideways, position, window, cx).is_some()
}

pub fn grip_at(position: Point<Pixels>, window: &Window, cx: &App) -> Option<Point<Pixels>> {
    found(Kind::Grip, position, window, cx).map(|bounds| bounds.center())
}

fn record(kind: Kind, bounds: Bounds<Pixels>) {
    REGIONS.with_borrow_mut(|regions| regions.push((kind, cover::mark(), bounds)));
}

fn found(kind: Kind, position: Point<Pixels>, window: &Window, cx: &App) -> Option<Bounds<Pixels>> {
    let finger = Bounds::new(position, size(px(1.), px(1.)));
    REGIONS.with_borrow(|regions| {
        regions
            .iter()
            .rev()
            .filter(|(marked, _, bounds)| *marked == kind && bounds.contains(&position))
            .find(|(_, at, _)| !cover::covered(*at, finger, window, cx))
            .map(|(_, _, bounds)| *bounds)
    })
}

fn mark(kind: Kind) -> gpui::Canvas<()> {
    marked_when(kind, || true)
}

fn marked_when(kind: Kind, present: impl FnOnce() -> bool + 'static) -> gpui::Canvas<()> {
    canvas(
        move |bounds, window, _| {
            if present() {
                record(kind, bounds.intersect(&window.content_mask().bounds));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
}
