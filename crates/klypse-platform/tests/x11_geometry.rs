use klypse_platform::{Rect, bgra_to_rgba, normalize_selection};

#[test]
fn reversed_drag_is_normalized() {
    assert_eq!(
        normalize_selection((100, 80), (20, 30)),
        Rect {
            x: 20,
            y: 30,
            width: 80,
            height: 50,
        }
    );
}

#[test]
fn selection_coordinates_are_saturated_to_protocol_limits() {
    assert_eq!(
        normalize_selection((i32::MIN, i32::MIN), (i32::MAX, i32::MAX)),
        Rect {
            x: i32::MIN,
            y: i32::MIN,
            width: u32::MAX,
            height: u32::MAX,
        }
    );
}

#[test]
fn bgra_pixels_become_rgba() {
    let rgba = bgra_to_rgba(&[0x10, 0x20, 0x30, 0xff]).unwrap();

    assert_eq!(rgba, [0x30, 0x20, 0x10, 0xff]);
}

#[test]
fn bgra_conversion_rejects_partial_pixels() {
    assert!(bgra_to_rgba(&[0, 1, 2]).is_err());
}
