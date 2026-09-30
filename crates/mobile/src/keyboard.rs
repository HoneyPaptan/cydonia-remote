use gui::model::keyboard;

fn touch_screen() -> bool {
    web_sys::window()
        .and_then(|window| js_sys::Reflect::get(&window, &"cydoniaTouch".into()).ok())
        .and_then(|touch| touch.as_bool())
        .unwrap_or(false)
}

pub fn install() {
    keyboard::set_enter_breaks_line(touch_screen());
}
