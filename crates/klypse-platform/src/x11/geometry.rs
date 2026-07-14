#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn normalize_selection(start: (i32, i32), end: (i32, i32)) -> Rect {
    Rect {
        x: start.0.min(end.0),
        y: start.1.min(end.1),
        width: start.0.abs_diff(end.0),
        height: start.1.abs_diff(end.1),
    }
}
