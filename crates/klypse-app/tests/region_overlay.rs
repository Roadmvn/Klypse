use klypse_app::ui::region_overlay::{OverlayPlacement, RegionSelectionState, to_root_coordinates};
use klypse_platform::Rect;

#[test]
fn click_selects_hovered_frame_but_drag_takes_precedence() {
    let frame = Rect {
        x: 100,
        y: 80,
        width: 600,
        height: 400,
    };
    let mut state = RegionSelectionState::default();
    state.hover(Some(frame));
    assert_eq!(state.confirm(), Some(frame));
    state.begin((150, 120));
    state.update((152, 121));
    assert_eq!(state.confirm(), Some(frame));
    state.update((250, 200));
    assert_eq!(
        state.confirm(),
        Some(Rect {
            x: 150,
            y: 120,
            width: 100,
            height: 80
        })
    );
    state.cancel();
    assert_eq!(state.confirm(), None);
}

#[test]
fn window_detection_picks_the_frontmost_frame_and_excludes_its_far_edge() {
    use klypse_app::ui::region_overlay::window_at_point;
    let front = Rect {
        x: 100,
        y: 100,
        width: 200,
        height: 200,
    };
    let back = Rect {
        x: 0,
        y: 0,
        width: 500,
        height: 500,
    };
    assert_eq!(window_at_point(&[front, back], (150, 150)), Some(front));
    assert_eq!(window_at_point(&[front, back], (300, 150)), Some(back));
    assert_eq!(window_at_point(&[front, back], (600, 600)), None);
}

#[test]
fn region_selection_normalizes_reverse_drag() {
    let mut state = RegionSelectionState::default();
    state.begin((100, 80));
    state.update((20, 30));

    assert_eq!(
        state.confirm(),
        Some(Rect {
            x: 20,
            y: 30,
            width: 80,
            height: 50,
        })
    );
}

#[test]
fn escape_cancels_and_zero_area_is_not_confirmed() {
    let mut state = RegionSelectionState::default();
    state.begin((10, 10));
    state.update((10, 10));
    assert_eq!(state.confirm(), None);

    state.update((20, 20));
    state.cancel();
    assert_eq!(state.confirm(), None);
}

#[test]
fn selection_is_shifted_by_the_hosting_monitor_origin() {
    let selection = Rect {
        x: 40,
        y: 10,
        width: 500,
        height: 350,
    };
    let placement = OverlayPlacement {
        origin: (2560, 0),
        scale: 1,
    };

    assert_eq!(
        to_root_coordinates(selection, placement),
        Rect {
            x: 2600,
            y: 10,
            width: 500,
            height: 350,
        }
    );
}

#[test]
fn selection_scales_to_device_pixels() {
    let selection = Rect {
        x: 10,
        y: 20,
        width: 100,
        height: 200,
    };
    let placement = OverlayPlacement {
        origin: (0, 90),
        scale: 2,
    };

    assert_eq!(
        to_root_coordinates(selection, placement),
        Rect {
            x: 20,
            y: 220,
            width: 200,
            height: 400,
        }
    );
}
