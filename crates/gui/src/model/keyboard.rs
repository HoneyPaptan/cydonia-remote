use std::cell::Cell;

thread_local! {
    static ENTER_BREAKS_LINE: Cell<bool> = const { Cell::new(false) };
}

pub fn enter_breaks_line() -> bool {
    ENTER_BREAKS_LINE.with(Cell::get)
}

pub fn set_enter_breaks_line(on: bool) {
    ENTER_BREAKS_LINE.with(|held| held.set(on));
}
