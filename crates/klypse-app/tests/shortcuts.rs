use klypse_app::ui::shortcuts::parse_xfce_bindings;

#[test]
fn reads_only_real_klypse_desktop_bindings() {
    let rows = parse_xfce_bindings(
        "/commands/custom/<Primary>Print   klypse capture area\n/commands/custom/<Super>Print /home/test/.local/bin/klypse capture screen --delay 5\n/commands/default/Print klypse capture screen\n/commands/custom/<Alt>F2 xfrun4\n/commands/custom/override true",
    );
    assert_eq!(
        rows,
        vec![
            ("<Primary>Print".into(), "klypse capture area".into()),
            (
                "<Super>Print".into(),
                "klypse capture screen --delay 5".into()
            ),
        ]
    );
}
