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
fn source_prefers_existing_user_policy_then_system_desktop_policy() {
    let root = tempfile::tempdir().unwrap();
    let user = root.path().join("user");
    let system = root.path().join("system");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::create_dir_all(&system).unwrap();
    let system_policy = system.join("niri-portals.conf");
    std::fs::write(&system_policy, "[preferred]\ndefault=gtk\n").unwrap();

    assert_eq!(
        portal_config::find_policy_source(&user, std::slice::from_ref(&system), "niri"),
        Some(system_policy)
    );

    let user_policy = user.join("portals.conf");
    std::fs::write(&user_policy, "[preferred]\ndefault=gnome;gtk;\n").unwrap();
    assert_eq!(
        portal_config::find_policy_source(&user, &[system], "niri"),
        Some(user_policy)
    );
}
