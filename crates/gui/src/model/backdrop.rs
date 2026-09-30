use crate::model::settings;
use bezel::gpui::{App, Global, Hsla, RenderImage};
use image::{Frame, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

const MAX_SIDE: u32 = 1600;
const SOURCE_FILE: &str = "source";
const FOLDER: &str = "backdrop";

pub const INTENSITY: (f32, f32) = (0.1, 0.9);
pub const DEFAULT_INTENSITY: f32 = 0.5;

const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

const GLYPHS: [[u8; 7]; 10] = [
    [0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 4, 0],
    [0, 4, 0, 0, 4, 0, 0],
    [0, 0, 0, 14, 0, 0, 0],
    [0, 0, 14, 0, 14, 0, 0],
    [0, 4, 4, 31, 4, 4, 0],
    [0, 21, 14, 31, 14, 21, 0],
    [10, 10, 31, 10, 31, 10, 10],
    [17, 2, 4, 4, 8, 16, 17],
    [14, 17, 23, 21, 23, 16, 14],
];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    None,
    Dither,
    Ascii,
    Halftone,
    Scanlines,
}

impl Effect {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Dither,
        Self::Ascii,
        Self::Halftone,
        Self::Scanlines,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Dither => "Dither",
            Self::Ascii => "ASCII",
            Self::Halftone => "Halftone",
            Self::Scanlines => "Scanlines",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Dither => "dither",
            Self::Ascii => "ascii",
            Self::Halftone => "halftone",
            Self::Scanlines => "scanlines",
        }
    }

    const fn follows_paper(self) -> bool {
        !matches!(self, Self::None | Self::Dither)
    }
}

pub fn clamp_intensity(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(INTENSITY.0, INTENSITY.1)
    } else {
        DEFAULT_INTENSITY
    }
}

pub struct Source {
    width: u32,
    height: u32,
    colors: Vec<[u8; 4]>,
    lumas: Vec<u8>,
}

impl Source {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let image = image::load_from_memory(bytes)
            .ok()?
            .thumbnail(MAX_SIDE, MAX_SIDE);
        let lumas = image.to_luma8().into_raw();
        let rgba = image.to_rgba8();
        Some(Self {
            width: rgba.width(),
            height: rgba.height(),
            colors: rgba.pixels().map(|pixel| pixel.0).collect(),
            lumas,
        })
    }

    fn index(&self, x: u32, y: u32) -> usize {
        (y.min(self.height - 1) * self.width + x.min(self.width - 1)) as usize
    }

    fn color(&self, x: u32, y: u32) -> [u8; 4] {
        self.colors[self.index(x, y)]
    }

    fn luma(&self, x: u32, y: u32) -> u8 {
        self.lumas[self.index(x, y)]
    }

    fn paint(&self, pixel: impl Fn(u32, u32) -> [u8; 4]) -> RgbaImage {
        RgbaImage::from_fn(self.width, self.height, |x, y| Rgba(pixel(x, y)))
    }

    pub fn render(&self, effect: Effect, light: bool) -> RgbaImage {
        match effect {
            Effect::None => self.paint(|x, y| self.color(x, y)),
            Effect::Dither => self.dither(),
            Effect::Ascii => self.ascii(light),
            Effect::Halftone => self.halftone(light),
            Effect::Scanlines => self.scanlines(light),
        }
    }

    fn scanlines(&self, light: bool) -> RgbaImage {
        self.paint(|x, y| {
            let [r, g, b, a] = self.color(x, y);
            let gain = if y % 3 == 0 { 0.52 } else { 1.0 };
            let channel = |value: u8| {
                let value = f32::from(value);
                match light {
                    true => value + (255.0 - value) * (1.0 - gain),
                    false => value * gain,
                }
                .round() as u8
            };
            [channel(r), channel(g), channel(b), a]
        })
    }

    fn ascii(&self, light: bool) -> RgbaImage {
        self.paint(|x, y| {
            let (sx, sy) = (x / 6 * 6 + 3, y / 8 * 8 + 4);
            let sample = self.luma(sx, sy);
            let density = if light { 255 - sample } else { sample };
            let glyph = ((f32::from(density) / 255.0).sqrt() * 9.0) as usize;
            let ink = x % 6 < 5 && y % 8 < 7 && GLYPHS[glyph][(y % 8) as usize] & (1 << (4 - x % 6)) != 0;
            let [r, g, b, a] = self.color(x, y);
            let [cr, cg, cb, _] = self.color(sx, sy);
            let paper = if light { 255.0 } else { 0.0 };
            let mix = |base: u8, glyph: u8| {
                let top = if ink { f32::from(glyph) } else { paper };
                (f32::from(base) * 0.6 + top * 0.4) as u8
            };
            [mix(r, cr), mix(g, cg), mix(b, cb), a]
        })
    }

    fn halftone(&self, light: bool) -> RgbaImage {
        let paper: u8 = if light { 255 } else { 0 };
        let mut pixels = RgbaImage::from_pixel(self.width, self.height, Rgba([paper, paper, paper, 255]));
        for y in (0..self.height).step_by(4) {
            for x in (0..self.width).step_by(4) {
                let luma = self.luma(x, y);
                let luma = if light { 255 - luma } else { luma };
                let radius = 2.0 * (0.3 + 0.7 * (f32::from(luma) / 255.0).sqrt());
                let [r, g, b, a] = self.color(x + 2, y + 2);
                for dy in 0..4.min(self.height - y) {
                    for dx in 0..4.min(self.width - x) {
                        let distance = ((dx as f32 - 1.5).powi(2) + (dy as f32 - 1.5).powi(2)).sqrt();
                        let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0) * f32::from(a) / 255.0;
                        let [sr, sg, sb, sa] = self.color(x + dx, y + dy);
                        let blend = |source: u8, dot: u8| {
                            let dotted = f32::from(dot) * coverage + f32::from(paper) * (1.0 - coverage);
                            (f32::from(source) * 0.6 + dotted * 0.4) as u8
                        };
                        pixels.put_pixel(x + dx, y + dy, Rgba([blend(sr, r), blend(sg, g), blend(sb, b), sa]));
                    }
                }
            }
        }
        pixels
    }

    fn dither(&self) -> RgbaImage {
        self.paint(|x, y| {
            let threshold = BAYER[(y / 2 % 4) as usize][(x / 2 % 4) as usize];
            dither_color(self.color(x / 2 * 2 + 1, y / 2 * 2 + 1), threshold)
        })
    }
}

fn dither_color([r, g, b, a]: [u8; 4], threshold: u8) -> [u8; 4] {
    let peak = f32::from(r.max(g).max(b));
    let bright = peak / 255.0 > (f32::from(threshold) + 0.5) / 16.0;
    let gain = if bright { 255.0 / peak.max(1.0) } else { 0.08 };
    let scale = |value: u8| (f32::from(value) * gain).round().min(255.0) as u8;
    [scale(r), scale(g), scale(b), a]
}

fn artwork(mut image: RgbaImage) -> Arc<RenderImage> {
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new([Frame::new(image)]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    effect: Effect,
    light: bool,
}

impl Key {
    fn new(effect: Effect, light: bool) -> Self {
        Self {
            effect,
            light: light && effect.follows_paper(),
        }
    }
}

pub struct Backdrop {
    source: Option<Arc<Source>>,
    effect: Effect,
    intensity: f32,
    ready: Option<(Key, Arc<RenderImage>)>,
    pending: Option<Key>,
}

impl Global for Backdrop {}

fn folder() -> Option<PathBuf> {
    Some(settings::dir().ok()?.join(FOLDER))
}

fn stored() -> Option<PathBuf> {
    Some(folder()?.join(SOURCE_FILE))
}

pub fn init(effect: Effect, intensity: f32, cx: &mut App) {
    cx.set_global(Backdrop {
        source: None,
        effect,
        intensity: clamp_intensity(intensity),
        ready: None,
        pending: None,
    });
    if let Some(path) = stored().filter(|path| path.exists()) {
        load(path, false, cx);
    }
}

pub fn choose(path: PathBuf, cx: &mut App) {
    load(path, true, cx);
}

pub fn use_bytes(bytes: Vec<u8>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let source = cx
            .background_executor()
            .spawn(async move { Source::decode(&bytes).map(Arc::new) })
            .await;
        cx.update(|cx| adopt(source, cx));
    })
    .detach();
}

fn load(path: PathBuf, keep: bool, cx: &mut App) {
    cx.spawn(async move |cx| {
        let source = cx
            .background_executor()
            .spawn(async move {
                let bytes = std::fs::read(&path).ok()?;
                let source = Source::decode(&bytes).map(Arc::new)?;
                if keep {
                    persist(&bytes);
                }
                Some(source)
            })
            .await;
        cx.update(|cx| adopt(source, cx));
    })
    .detach();
}

fn persist(bytes: &[u8]) -> Option<()> {
    std::fs::create_dir_all(folder()?).ok()?;
    std::fs::write(stored()?, bytes).ok()
}

fn adopt(source: Option<Arc<Source>>, cx: &mut App) {
    let Some(source) = source else {
        return;
    };
    let held = cx.global_mut::<Backdrop>();
    held.source = Some(source);
    held.ready = None;
    held.pending = None;
    cx.refresh_windows();
}

pub fn clear(cx: &mut App) {
    if let Some(path) = stored() {
        let _ = std::fs::remove_file(path);
    }
    let held = cx.global_mut::<Backdrop>();
    held.source = None;
    held.ready = None;
    held.pending = None;
    cx.refresh_windows();
}

pub fn set_effect(effect: Effect, cx: &mut App) {
    cx.global_mut::<Backdrop>().effect = effect;
    cx.refresh_windows();
}

pub fn set_intensity(intensity: f32, cx: &mut App) {
    cx.global_mut::<Backdrop>().intensity = clamp_intensity(intensity);
    cx.refresh_windows();
}

pub fn is_set(cx: &App) -> bool {
    cx.try_global::<Backdrop>()
        .is_some_and(|held| held.source.is_some())
}

pub fn veil(base: Hsla, cx: &App) -> Option<Hsla> {
    let held = cx.try_global::<Backdrop>()?;
    held.source.as_ref()?;
    Some(Hsla {
        a: 1.0 - held.intensity,
        ..base
    })
}

pub fn frame(light: bool, cx: &mut App) -> Option<Arc<RenderImage>> {
    let held = cx.try_global::<Backdrop>()?;
    let source = held.source.clone()?;
    let key = Key::new(held.effect, light);
    let shown = held.ready.as_ref().map(|(_, image)| image.clone());
    if held.ready.as_ref().is_some_and(|(ready, _)| *ready == key) || held.pending == Some(key) {
        return shown;
    }
    cx.global_mut::<Backdrop>().pending = Some(key);
    cx.spawn(async move |cx| {
        let image = cx
            .background_executor()
            .spawn(async move { artwork(source.render(key.effect, key.light)) })
            .await;
        cx.update(|cx| {
            let held = cx.global_mut::<Backdrop>();
            if held.pending == Some(key) {
                held.ready = Some((key, image));
                held.pending = None;
            }
            cx.refresh_windows();
        });
    })
    .detach();
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Source {
        Source {
            width: 60,
            height: 32,
            colors: vec![[128, 64, 32, 200]; 1920],
            lumas: vec![128; 1920],
        }
    }

    fn brightness(image: &RgbaImage) -> u64 {
        image
            .pixels()
            .map(|pixel| pixel.0[..3].iter().map(|channel| u64::from(*channel)).sum::<u64>())
            .sum()
    }

    #[test]
    fn every_effect_keeps_the_source_size_and_alpha() {
        let source = fixture();
        for effect in Effect::ALL {
            let image = source.render(effect, false);
            assert_eq!(image.dimensions(), (60, 32), "{effect:?}");
            assert!(image.pixels().all(|pixel| pixel.0[3] == 200), "{effect:?}");
        }
    }

    #[test]
    fn light_paper_is_brighter_and_keeps_the_source_warm() {
        let source = fixture();
        for effect in [Effect::Ascii, Effect::Halftone, Effect::Scanlines] {
            let light = source.render(effect, true);
            let dark = source.render(effect, false);
            assert!(brightness(&light) > brightness(&dark), "{effect:?}");
            assert!(light.pixels().all(|pixel| pixel.0[0] >= pixel.0[1] && pixel.0[1] >= pixel.0[2]));
        }
    }

    #[test]
    fn dither_ignores_paper_and_the_key_reflects_it() {
        assert_eq!(Key::new(Effect::Dither, true), Key::new(Effect::Dither, false));
        assert_ne!(Key::new(Effect::Ascii, true), Key::new(Effect::Ascii, false));
    }

    #[test]
    fn odd_sized_images_render_without_reading_past_the_edge() {
        let source = Source {
            width: 7,
            height: 5,
            colors: vec![[200, 100, 50, 255]; 35],
            lumas: vec![100; 35],
        };
        for effect in Effect::ALL {
            assert_eq!(source.render(effect, true).dimensions(), (7, 5));
        }
    }

    #[test]
    fn intensity_stays_inside_its_range() {
        assert_eq!(clamp_intensity(f32::NAN), DEFAULT_INTENSITY);
        assert_eq!(clamp_intensity(5.0), INTENSITY.1);
        assert_eq!(clamp_intensity(-1.0), INTENSITY.0);
    }
}
