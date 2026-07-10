use super::*;

#[test]
fn updates_only_filechooser_preference() {
    let original = r#"[preferred]
default=gnome;gtk;
org.freedesktop.impl.portal.FileChooser=gtk
org.freedesktop.impl.portal.Settings=gtk
"#;

    let updated = portal_config::set_filechooser_backend(original);

    assert_eq!(
        updated,
        r#"[preferred]
default=gnome;gtk;
org.freedesktop.impl.portal.FileChooser=termfilechooser
org.freedesktop.impl.portal.Settings=gtk
"#
    );
}

#[test]
fn adds_missing_preferred_section_key() {
    let original = "[preferred]\ndefault=gtk\n\n[other]\nvalue=true\n";

    let updated = portal_config::set_filechooser_backend(original);

    assert_eq!(
        updated,
        "[preferred]\ndefault=gtk\norg.freedesktop.impl.portal.FileChooser=termfilechooser\n\n[other]\nvalue=true\n"
    );
}

#[test]
fn source_follows_directory_then_desktop_then_generic_precedence() {
    let root = tempfile::tempdir().unwrap();
    let high_priority = root.path().join("high");
    let low_priority = root.path().join("low");
    std::fs::create_dir_all(&high_priority).unwrap();
    std::fs::create_dir_all(&low_priority).unwrap();
    let high_generic = high_priority.join("portals.conf");
    std::fs::write(&high_generic, "[preferred]\ndefault=gtk\n").unwrap();
    std::fs::write(
        low_priority.join("niri-portals.conf"),
        "[preferred]\ndefault=gnome\n",
    )
    .unwrap();

    let source = portal_config::find_policy_source(
        &[high_priority, low_priority],
        &["niri".into(), "gnome".into()],
    );

    assert_eq!(source, Some(high_generic));
}

#[test]
fn source_checks_every_desktop_token_in_order() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path();
    let gnome_policy = directory.join("gnome-portals.conf");
    std::fs::write(&gnome_policy, "[preferred]\ndefault=gnome\n").unwrap();
    std::fs::write(directory.join("portals.conf"), "[preferred]\ndefault=gtk\n").unwrap();

    let source = portal_config::find_policy_source(
        &[directory.to_path_buf()],
        &["niri".into(), "gnome".into()],
    );

    assert_eq!(source, Some(gnome_policy));
}

#[test]
fn desktop_tokens_are_lowercased_and_empty_tokens_are_removed() {
    assert_eq!(
        portal_config::parse_desktops("NIRI::GNOME:"),
        vec!["niri", "gnome"]
    );
}

#[test]
fn policy_directories_follow_xdg_precedence() {
    let directories = portal_config::build_policy_directories(
        PathBuf::from("/user/config"),
        vec![
            PathBuf::from("/system/config-a"),
            PathBuf::from("/system/config-b"),
        ],
        PathBuf::from("/user/data"),
        vec![PathBuf::from("/system/data")],
    );

    assert_eq!(
        directories,
        vec![
            PathBuf::from("/user/config/xdg-desktop-portal"),
            PathBuf::from("/system/config-a/xdg-desktop-portal"),
            PathBuf::from("/system/config-b/xdg-desktop-portal"),
            PathBuf::from("/etc/xdg-desktop-portal"),
            PathBuf::from("/user/data/xdg-desktop-portal"),
            PathBuf::from("/system/data/xdg-desktop-portal"),
            PathBuf::from("/usr/share/xdg-desktop-portal"),
        ]
    );
}
