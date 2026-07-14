use klypse_app::ui::region_overlay::RegionSelectionState;
use klypse_platform::Rect;

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
