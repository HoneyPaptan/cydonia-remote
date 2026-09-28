use crate::model::pictures::{self, Fit, Placed};
use bezel::{
    gpui::{
        AnyElement, App, Bounds, Element, ElementId, GlobalElementId, Image, ImageSource,
        InspectorElementId, IntoElement, LayoutId, Pixels, Window,
    },
    ui::cover::{self, Mark},
};
use std::{
    hash::{DefaultHasher, Hash as _, Hasher as _},
    sync::Arc,
};

pub fn framed(
    source: &ImageSource,
    fit: Fit,
    radius: Pixels,
    child: impl IntoElement,
) -> AnyElement {
    match (pictures::get(), source) {
        (Some(_), ImageSource::Image(image)) => Framed {
            image: image.clone(),
            fit,
            radius,
            child: child.into_any_element(),
        }
        .into_any_element(),
        _ => child.into_any_element(),
    }
}

struct Framed {
    image: Arc<Image>,
    fit: Fit,
    radius: Pixels,
    child: AnyElement,
}

fn key(id: &GlobalElementId) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

impl IntoElement for Framed {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Framed {
    type RequestLayoutState = ();
    type PrepaintState = Mark;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::NamedInteger("picture".into(), self.image.id()))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Mark {
        let mark = cover::mark();
        self.child.prepaint(window, cx);
        mark
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        mark: &mut Mark,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        let (Some(id), Some(pictures)) = (id, pictures::get()) else {
            return;
        };
        let clip = window.content_mask().bounds.intersect(&bounds);
        let holes = cover::over(*mark, clip, window, cx);
        if clip.is_empty() || holes.contains(&clip) {
            return;
        }
        pictures.place(Placed {
            key: key(id),
            image: &self.image,
            bounds,
            clip,
            holes,
            radius: self.radius,
            fit: self.fit,
        });
    }
}
