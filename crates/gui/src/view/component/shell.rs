use crate::model::{relay, typography};
use bezel::{
    gpui::{
        self, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding,
        Pixels, ScrollWheelEvent, SharedString, Task, Window, actions, canvas, div, prelude::*, px,
    },
    theme::{Appearance, TextStyle, Theme, Typeset},
    ui::{
        input::TextField,
        widgets::{ButtonStyle, Buttons as _},
    },
};
use futures::StreamExt as _;
use remote::proto::{BOLD, Color, DIM, ITALIC, Run, Screen, ShellInput, UNDERLINE};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
};

actions!(cydonia_shell, [Submit]);

const CONTEXT: &str = "CydoniaShell";
const LINE: f32 = 1.3;
const INSET: f32 = 8.;

const KEYS: [(&str, &str); 8] = [
    ("Esc", "\x1b"),
    ("Tab", "\t"),
    ("^C", "\x03"),
    ("^D", "\x04"),
    ("\u{2191}", "\x1b[A"),
    ("\u{2193}", "\x1b[B"),
    ("\u{2190}", "\x1b[D"),
    ("\u{2192}", "\x1b[C"),
];

const ANSI_DARK: [(u8, u8, u8); 16] = [
    (0x24, 0x24, 0x24),
    (0xf8, 0x71, 0x71),
    (0x4a, 0xde, 0x80),
    (0xfa, 0xcc, 0x15),
    (0x60, 0xa5, 0xfa),
    (0xc0, 0x84, 0xfc),
    (0x22, 0xd3, 0xee),
    (0xd4, 0xd4, 0xd8),
    (0x52, 0x52, 0x5b),
    (0xfc, 0xa5, 0xa5),
    (0x86, 0xef, 0xac),
    (0xfd, 0xe0, 0x47),
    (0x93, 0xc5, 0xfd),
    (0xd8, 0xb4, 0xfe),
    (0x67, 0xe8, 0xf9),
    (0xfa, 0xfa, 0xfa),
];

const ANSI_LIGHT: [(u8, u8, u8); 16] = [
    (0x1f, 0x1f, 0x1f),
    (0xdc, 0x26, 0x26),
    (0x16, 0xa3, 0x4a),
    (0xb4, 0x53, 0x09),
    (0x25, 0x63, 0xeb),
    (0x93, 0x33, 0xea),
    (0x0e, 0x74, 0x90),
    (0x3f, 0x3f, 0x46),
    (0x71, 0x71, 0x7a),
    (0xb9, 0x1c, 0x1c),
    (0x15, 0x80, 0x3d),
    (0x92, 0x40, 0x0e),
    (0x1d, 0x4e, 0xd8),
    (0x7e, 0x22, 0xce),
    (0x15, 0x5e, 0x75),
    (0x18, 0x18, 0x1b),
];

pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new("enter", Submit, Some(CONTEXT))]
}

pub struct Exited;
pub struct DirectoryChanged;

pub struct Terminal {
    pub(crate) directory: PathBuf,
    screen: Screen,
    input: Option<Rc<dyn Fn(ShellInput)>>,
    field: Entity<TextField>,
    focus: FocusHandle,
    size: Rc<Cell<(u16, u16)>>,
    sent: (u16, u16),
    scrolled: f32,
    ended: bool,
    _screens: Option<Task<()>>,
}

impl EventEmitter<Exited> for Terminal {}
impl EventEmitter<DirectoryChanged> for Terminal {}

fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    gpui::Rgba {
        r: r as f32 / 255.,
        g: g as f32 / 255.,
        b: b as f32 / 255.,
        a: 1.,
    }
    .into()
}

fn paint(color: Color, fallback: Hsla, appearance: Appearance) -> Hsla {
    match color {
        Color::Default => fallback,
        Color::Indexed(index) => {
            let table = match appearance {
                Appearance::Dark => ANSI_DARK,
                Appearance::Light => ANSI_LIGHT,
            };
            let (r, g, b) = table[index as usize % 16];
            rgb(r, g, b)
        }
        Color::Rgb(r, g, b) => rgb(r, g, b),
    }
}

impl Terminal {
    pub fn new(cwd: &Path, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            TextField::new(cx)
                .with_key_context(CONTEXT)
                .with_placeholder("Type a command, then Enter")
        });
        let mut this = Self {
            directory: cwd.to_path_buf(),
            screen: Screen::default(),
            input: None,
            field,
            focus: cx.focus_handle(),
            size: Rc::new(Cell::new((0, 0))),
            sent: (0, 0),
            scrolled: 0.,
            ended: false,
            _screens: None,
        };
        this.connect(cx);
        this
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        let Some(opened) = relay::shells().and_then(|shells| {
            shells.open(&self.directory.to_string_lossy(), 80, 24)
        }) else {
            self.ended = true;
            return;
        };
        self.input = Some(opened.input);
        let mut screens = opened.screens;
        self._screens = Some(cx.spawn(async move |this, cx| {
            while let Some(screen) = screens.next().await {
                if this.update(cx, |this, cx| this.show(screen, cx)).is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.ended = true;
                this.input = None;
                cx.emit(Exited);
                cx.notify();
            });
        }));
    }

    fn show(&mut self, screen: Screen, cx: &mut Context<Self>) {
        if let Some(directory) = screen.directory.as_deref().map(PathBuf::from)
            && directory != self.directory
        {
            self.directory = directory;
            cx.emit(DirectoryChanged);
        }
        self.screen = screen;
        cx.notify();
    }

    fn send(&self, input: ShellInput) {
        if let Some(send) = &self.input {
            send(input);
        }
    }

    fn keys(&self, text: &str) {
        self.send(ShellInput::Keys {
            text: text.to_owned(),
        });
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        let typed = self.field.read(cx).content().to_string();
        self.keys(&format!("{typed}\r"));
        self.field.update(cx, |field, cx| field.set_content("", cx));
    }

    fn resize(&mut self) {
        let size = self.size.get();
        if size != self.sent && size.0 > 0 && size.1 > 0 {
            self.sent = size;
            self.send(ShellInput::Resize {
                cols: size.0,
                rows: size.1,
            });
        }
    }

    fn wheel(&mut self, event: &ScrollWheelEvent, line: Pixels, cx: &mut Context<Self>) {
        self.scrolled += f32::from(event.delta.pixel_delta(line).y);
        let lines = (self.scrolled / f32::from(line)).trunc();
        if lines != 0. {
            self.scrolled -= lines * f32::from(line);
            self.send(ShellInput::Scroll {
                lines: lines as i32,
            });
        }
        cx.stop_propagation();
    }

    fn row(runs: &[Run], theme: &Theme, line: Pixels) -> gpui::Div {
        div()
            .flex()
            .flex_none()
            .h(line)
            .whitespace_nowrap()
            .overflow_hidden()
            .children(runs.iter().map(|run| {
                div()
                    .flex_none()
                    .text_color(paint(run.fg, theme.text, theme.appearance))
                    .when(run.bg != Color::Default, |text| {
                        text.bg(paint(run.bg, theme.surface, theme.appearance))
                    })
                    .when(run.style & BOLD != 0, |text| {
                        text.font_weight(gpui::FontWeight::BOLD)
                    })
                    .when(run.style & DIM != 0, |text| text.opacity(0.6))
                    .when(run.style & ITALIC != 0, |text| text.italic())
                    .when(run.style & UNDERLINE != 0, |text| text.underline())
                    .child(SharedString::from(run.text.clone()))
            }))
    }

    fn key_row(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .gap(px(4.))
            .children(KEYS.iter().map(|(label, bytes)| {
                let bytes = *bytes;
                theme
                    .button(*label, ButtonStyle::Ghost, None)
                    .id(SharedString::from(format!("shell-key-{label}")))
                    .on_click(cx.listener(move |this, _, _, _| this.keys(bytes)))
            }))
    }
}

impl Focusable for Terminal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.ended {
            self.focus.clone()
        } else {
            self.field.focus_handle(cx)
        }
    }
}

impl Render for Terminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.resize();
        let theme = Theme::of(cx).clone();
        if self.ended && self.input.is_none() && self.screen.rows.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .track_focus(&self.focus)
                .text_style(TextStyle::Caption)
                .text_color(theme.text_muted)
                .child("The laptop did not open a terminal here.")
                .into_any_element();
        }
        let size = px(typography::terminal_size(cx));
        let line = size * LINE;
        let font = gpui::font(theme.font_mono.clone());
        let cell = window
            .text_system()
            .advance(window.text_system().resolve_font(&font), size, 'm')
            .map(|advance| advance.width)
            .unwrap_or(size * 0.6);
        let measured = self.size.clone();
        let this = cx.entity().downgrade();
        let cursor = self.screen.cursor.filter(|_| !self.ended);
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .child(
                div()
                    .id("shell-screen")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .p(px(INSET))
                    .font_family(theme.font_mono.clone())
                    .text_size(size)
                    .line_height(line)
                    .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                        this.wheel(event, line, cx)
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.field.focus_handle(cx), cx);
                        window.request_virtual_keyboard();
                    }))
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                let cols = ((f32::from(bounds.size.width) - 2. * INSET)
                                    / f32::from(cell))
                                .floor()
                                .max(1.) as u16;
                                let rows = ((f32::from(bounds.size.height) - 2. * INSET)
                                    / f32::from(line))
                                .floor()
                                .max(1.) as u16;
                                if measured.get() != (cols, rows) {
                                    measured.set((cols, rows));
                                    let _ = this.update(cx, |_, cx| cx.notify());
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .children(
                        self.screen
                            .rows
                            .iter()
                            .map(|runs| Self::row(runs, &theme, line)),
                    )
                    .children(cursor.map(|(row, col)| {
                        div()
                            .absolute()
                            .left(px(INSET) + cell * col as f32)
                            .top(px(INSET) + line * row as f32)
                            .w(cell)
                            .h(line)
                            .bg(theme.text.opacity(0.5))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(4.))
                    .p(px(6.))
                    .border_t_1()
                    .border_color(theme.border)
                    .child(self.key_row(&theme, cx))
                    .child(
                        div()
                            .on_action(cx.listener(Self::submit))
                            .child(self.field.clone()),
                    ),
            )
            .into_any_element()
    }
}
