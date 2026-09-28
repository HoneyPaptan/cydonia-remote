use std::cell::RefCell;

use gpui::{Bounds, IntoElement, Pixels, Point, Styled, canvas, px};

const GRIP_REACH: f32 = 12.;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Sideways,
    Grip,
}

thread_local! {
    static REGIONS: RefCell<Vec<(Kind, Bounds<Pixels>)>> = const { RefCell::new(Vec::new()) };
}

pub fn forget() {
    REGIONS.with_borrow_mut(Vec::clear);
}

pub fn sideways() -> impl IntoElement {
    mark(Kind::Sideways).inset_0()
}

pub fn grip() -> impl IntoElement {
    let reach = px(-GRIP_REACH);
    mark(Kind::Grip).top(reach).left(reach).right(reach).bottom(reach)
}

pub fn scrolls_sideways_at(position: Point<Pixels>) -> bool {
    found(Kind::Sideways, position).is_some()
}

pub fn grip_at(position: Point<Pixels>) -> Option<Point<Pixels>> {
    found(Kind::Grip, position).map(|bounds| bounds.center())
}

fn found(kind: Kind, position: Point<Pixels>) -> Option<Bounds<Pixels>> {
    REGIONS.with_borrow(|regions| {
        regions
            .iter()
            .rev()
            .find(|(marked, bounds)| *marked == kind && bounds.contains(&position))
            .map(|(_, bounds)| *bounds)
    })
}

fn mark(kind: Kind) -> gpui::Canvas<()> {
    canvas(
        move |bounds, window, _| {
            let visible = bounds.intersect(&window.content_mask().bounds);
            REGIONS.with_borrow_mut(|regions| regions.push((kind, visible)));
        },
        |_, _, _, _| {},
    )
    .absolute()
}
