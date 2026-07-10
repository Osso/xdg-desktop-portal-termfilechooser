use super::*;
use std::os::unix::fs::PermissionsExt;

mod cancellation_tests;
mod portal_config_tests;

fn test_config() -> Config {
    Config::default()
}

fn owned_value(value: Value<'_>) -> OwnedValue {
    value.try_into().unwrap()
}

fn executable_script(name: &str, body: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[test]
fn defaults_have_terminal_chooser_and_home_fallback() {
    let config = Config::default();

    assert_eq!(
        config.filechooser.terminal,
        "kitty --class file-chooser --title"
    );
    assert_eq!(config.filechooser.chooser, "yazi --chooser-file");
    assert!(!config.filechooser.default_dir.is_empty());
}

#[test]
fn option_helpers_decode_bool_string_and_nul_terminated_bytes() {
    let mut options = HashMap::new();
    options.insert("multiple".into(), owned_value(Value::Bool(true)));
    options.insert(
        "current_folder".into(),
        owned_value(Value::Array(vec![b'/', b't', b'm', b'p', b'\0'].into())),
    );

    assert!(get_bool_option(&options, "multiple"));
    assert!(!get_bool_option(&options, "missing"));
    assert_eq!(
        get_bytes_option(&options, "current_folder"),
        Some("/tmp".into())
    );

    options.insert(
        "non_utf8_folder".into(),
        owned_value(Value::Array(
            vec![b'/', b't', b'm', b'p', b'/', 0xff, b'\0'].into(),
        )),
    );
    assert!(get_bytes_option(&options, "non_utf8_folder").is_some());

    options.insert("not_bool".into(), owned_value(Value::Str("true".into())));
    options.insert("not_bytes".into(), owned_value(Value::Str("/tmp".into())));
    assert!(!get_bool_option(&options, "not_bool"));
    assert_eq!(get_bytes_option(&options, "not_bytes"), None);
}

#[test]
fn parent_dir_helper_extracts_parent() {
    assert_eq!(
        parent_dir_from_path("/home/osso/Downloads/example.txt"),
        Some("/home/osso/Downloads".into())
    );
}

#[test]
fn chooser_args_insert_output_file() {
    let chooser = FileChooser::new(test_config());

    assert_eq!(
        chooser.build_chooser_args("/tmp/selection", Path::new("/home/osso")),
        vec!["yazi", "--chooser-file=/tmp/selection", "/home/osso"]
    );
}

#[test]
fn chooser_args_preserve_quoted_arguments() {
    let mut config = test_config();
    config.filechooser.chooser =
        "'/tmp/chooser with spaces' '--label=two words' --chooser-file".into();
    let chooser = FileChooser::new(config);

    assert_eq!(
        chooser.build_chooser_args("/tmp/selection", Path::new("/home/osso")),
        vec![
            OsString::from("/tmp/chooser with spaces"),
            OsString::from("--label=two words"),
            OsString::from("--chooser-file=/tmp/selection"),
            OsString::from("/home/osso"),
        ]
    );
}

#[test]
fn spawn_terminal_reports_success_and_failure_status() {
    let mut config = test_config();
    config.filechooser.terminal = "true".into();
    let chooser = FileChooser::new(config);

    assert!(
        chooser
            .spawn_terminal("ignored", &["true".into(), "ignored".into()])
            .is_ok()
    );

    let mut config = test_config();
    config.filechooser.terminal = "false".into();
    let chooser = FileChooser::new(config);
    let error = chooser
        .spawn_terminal("ignored", &["true".into(), "ignored".into()])
        .unwrap_err();

    assert!(error.starts_with("Terminal exited with:"));

    let mut config = test_config();
    config.filechooser.terminal = "/definitely/not/a/terminal".into();
    let chooser = FileChooser::new(config);
    let error = chooser
        .spawn_terminal("ignored", &["true".into(), "ignored".into()])
        .unwrap_err();
    assert!(error.starts_with("Failed to spawn terminal:"));
}

#[test]
fn spawn_terminal_supports_quoted_executable_path() {
    let terminal = executable_script("terminal with spaces", "#!/bin/sh\nexit 0\n");
    let mut config = test_config();
    config.filechooser.terminal = format!("'{}'", terminal.display());
    let chooser = FileChooser::new(config);

    assert!(
        chooser
            .spawn_terminal("ignored", &["true".into(), "ignored".into()])
            .is_ok()
    );
}

#[test]
fn spawn_terminal_rejects_empty_terminal_command() {
    let mut config = test_config();
    config.filechooser.terminal.clear();
    let chooser = FileChooser::new(config);

    let error = chooser
        .spawn_terminal("ignored", &["true".into(), "ignored".into()])
        .unwrap_err();

    assert_eq!(error, "Terminal command cannot be empty");
}

#[test]
fn dbus_introspection_exports_save_files_and_request_close() {
    use zbus::object_server::Interface;

    let chooser = FileChooser::new(test_config());
    let mut chooser_xml = String::new();
    chooser.introspect_to_writer(&mut chooser_xml, 0);
    assert!(chooser_xml.contains("<method name=\"SaveFiles\">"));

    let request = runtime::PortalRequest::new(CancellationToken::new());
    let mut request_xml = String::new();
    request.introspect_to_writer(&mut request_xml, 0);
    assert!(request_xml.contains("<method name=\"Close\">"));
}

#[test]
fn read_selections_prefers_explicit_output_paths() {
    let chooser = FileChooser::new(test_config());
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "/tmp/a.txt\n\n/tmp/b.txt\n").unwrap();

    let selections = chooser.read_selections(&tmp.path().to_string_lossy());

    assert_eq!(
        selections,
        vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")]
    );
}

#[test]
fn read_selections_returns_empty_without_explicit_acceptance() {
    let chooser = FileChooser::new(test_config());
    let tmp = tempfile::NamedTempFile::new().unwrap();

    let selections = chooser.read_selections(&tmp.path().to_string_lossy());

    assert!(selections.is_empty());
}

#[test]
fn run_chooser_returns_error_when_terminal_writes_no_selection() {
    let mut config = test_config();
    config.filechooser.terminal = "true".into();
    let chooser = FileChooser::new(config);

    let error = chooser
        .run_chooser(ChooserRequest {
            title: "ignored",
            start_path: Some(Path::new("/home/osso/Downloads")),
            multiple: false,
        })
        .unwrap_err();

    assert_eq!(error, "No files selected");
}

#[test]
fn run_chooser_returns_encoded_file_uris_from_terminal_selection() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
esac
done
printf '/tmp/space name #1.txt\n' > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);

    let selections = chooser
        .run_chooser(ChooserRequest {
            title: "Open",
            start_path: None,
            multiple: false,
        })
        .unwrap();

    assert_eq!(selections, vec!["file:///tmp/space%20name%20%231.txt"]);
}

#[test]
fn run_chooser_encodes_literal_percent_signs() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
esac
done
printf '/tmp/literal%%20name.txt\n' > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);

    let selections = chooser
        .run_chooser(ChooserRequest {
            title: "Open",
            start_path: None,
            multiple: false,
        })
        .unwrap();

    assert_eq!(selections, vec!["file:///tmp/literal%2520name.txt"]);
}

#[test]
fn run_chooser_encodes_non_utf8_path_bytes() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
esac
done
printf '/tmp/nonutf8-\377.txt\n' > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);

    let selections = chooser
        .run_chooser(ChooserRequest {
            title: "Open",
            start_path: None,
            multiple: false,
        })
        .unwrap();

    assert_eq!(selections, vec!["file:///tmp/nonutf8-%FF.txt"]);
}

#[test]
fn run_chooser_rejects_relative_paths() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
esac
done
printf 'relative.txt\n' > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);

    let error = chooser
        .run_chooser(ChooserRequest {
            title: "Open",
            start_path: None,
            multiple: false,
        })
        .unwrap_err();

    assert_eq!(error, "Chooser returned a relative path: relative.txt");
}

#[test]
fn run_chooser_rejects_multiple_results_for_single_selection() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
esac
done
printf '/tmp/one.txt\n/tmp/two.txt\n' > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);

    let error = chooser
        .run_chooser(ChooserRequest {
            title: "Open",
            start_path: None,
            multiple: false,
        })
        .unwrap_err();

    assert_eq!(
        error,
        "Chooser returned 2 paths for a single-selection request"
    );
}

#[test]
fn run_chooser_uses_requested_start_path() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
    /home/*) start="$arg" ;;
esac
done
printf '%s/report.txt\n' "$start" > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);

    let selections = chooser
        .run_chooser(ChooserRequest {
            title: "Save",
            start_path: Some(Path::new("/home/osso/Downloads")),
            multiple: false,
        })
        .unwrap();

    assert_eq!(selections, vec!["file:///home/osso/Downloads/report.txt"]);
}

#[test]
fn open_file_result_returns_uris_or_cancel_code() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
esac
done
printf '/tmp/open.txt\n' > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);

    let (code, result) = chooser.open_file_result("Open", HashMap::new());
    assert_eq!(code, 0);
    assert!(result.contains_key("uris"));

    let mut config = test_config();
    config.filechooser.terminal = "true".into();
    let chooser = FileChooser::new(config);
    let (code, result) = chooser.open_file_result("Open", HashMap::new());
    assert_eq!(code, 1);
    assert!(result.is_empty());

    let mut config = test_config();
    config.filechooser.terminal = "/definitely/not/a/terminal".into();
    let chooser = FileChooser::new(config);
    let (code, result) = chooser.open_file_result("Open", HashMap::new());
    assert_eq!(code, 2);
    assert!(result.is_empty());
}

#[test]
fn save_file_result_returns_explicit_selection_or_cancel_code() {
    let terminal = executable_script(
        "terminal",
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${arg#--chooser-file=}" ;;
esac
done
printf '/tmp/saved.txt\n' > "$out"
"#,
    );
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);
    let mut options = HashMap::new();
    options.insert(
        "current_name".into(),
        owned_value(Value::Str("saved.txt".into())),
    );

    let (code, result) = chooser.save_file_result("Save", options);
    assert_eq!(code, 0);
    assert!(result.contains_key("uris"));

    let mut config = test_config();
    config.filechooser.terminal = "true".into();
    let chooser = FileChooser::new(config);
    let (code, result) = chooser.save_file_result("Save", HashMap::new());
    assert_eq!(code, 1);
    assert!(result.is_empty());
}

#[test]
fn save_files_result_appends_each_filename_to_selected_directory() {
    let selected_dir = tempfile::tempdir().unwrap();
    let script = format!(
        r#"#!/bin/sh
for arg in "$@"; do
case "$arg" in
    --chooser-file=*) out="${{arg#--chooser-file=}}" ;;
esac
done
printf '{}\n' > "$out"
"#,
        selected_dir.path().display()
    );
    let terminal = executable_script("terminal", &script);
    let mut config = test_config();
    config.filechooser.terminal = terminal.to_string_lossy().into_owned();
    config.filechooser.chooser = "chooser --chooser-file".into();
    let chooser = FileChooser::new(config);
    let mut options = HashMap::new();
    let files: zbus::zvariant::Array = vec![
        b"first.txt\0".to_vec(),
        vec![b'n', b'o', b'n', b'u', b't', b'f', b'8', b'-', 0xff, 0],
    ]
    .into();
    options.insert("files".into(), owned_value(Value::Array(files)));

    let (code, result) = chooser.save_files_result("Save files", options);

    assert_eq!(code, 0);
    let uris = result.get("uris").unwrap().try_clone().unwrap();
    let uris = Vec::<String>::try_from(uris).unwrap();
    assert_eq!(
        uris,
        vec![
            format!("file://{}/first.txt", selected_dir.path().display()),
            format!("file://{}/nonutf8-%FF", selected_dir.path().display()),
        ]
    );
}

#[test]
fn save_files_result_rejects_missing_or_unsafe_filenames() {
    let chooser = FileChooser::new(test_config());
    let (missing_code, missing_result) = chooser.save_files_result("Save files", HashMap::new());
    assert_eq!(missing_code, 2);
    assert!(missing_result.is_empty());

    let mut options = HashMap::new();
    let files: zbus::zvariant::Array = vec![b"../escape.txt\0".to_vec()].into();
    options.insert("files".into(), owned_value(Value::Array(files)));
    let (unsafe_code, unsafe_result) = chooser.save_files_result("Save files", options);
    assert_eq!(unsafe_code, 2);
    assert!(unsafe_result.is_empty());
}

#[test]
fn build_uris_result_contains_string_array() {
    let result = build_uris_result(vec![
        "file:///tmp/a.txt".into(),
        "file:///tmp/file%20with%20space.txt".into(),
    ]);
    let uris = result.get("uris").unwrap().try_clone().unwrap();
    let uris = Vec::<String>::try_from(uris).unwrap();

    assert_eq!(
        uris,
        vec!["file:///tmp/a.txt", "file:///tmp/file%20with%20space.txt"]
    );
}

#[test]
fn load_config_reads_toml_and_falls_back_for_missing_or_invalid_files() {
    assert!(
        runtime::default_config_path()
            .map(|path| path.ends_with("xdg-desktop-portal-termfilechooser/config.toml"))
            .unwrap_or(true)
    );

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"
            [filechooser]
            terminal = "foot --title"
            chooser = "ranger --choosefile"
            default_dir = "/tmp"
        "#,
    )
    .unwrap();

    let config = runtime::load_config(Some(tmp.path().to_path_buf())).unwrap();

    assert_eq!(config.filechooser.terminal, "foot --title");
    assert_eq!(config.filechooser.chooser, "ranger --choosefile");
    assert_eq!(config.filechooser.default_dir, "/tmp");

    let missing = runtime::load_config(Some(tmp.path().with_extension("missing")));
    assert!(missing.unwrap_err().contains("does not exist"));

    std::fs::write(tmp.path(), "[filechooser").unwrap();
    let invalid = runtime::load_config(Some(tmp.path().to_path_buf()));
    assert!(invalid.unwrap_err().contains("Failed to parse config"));

    let read_error = runtime::load_config(Some(std::env::temp_dir()));
    assert!(read_error.unwrap_err().contains("Failed to read config"));
}

#[test]
fn load_config_rejects_empty_or_malformed_commands() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        r#"
            [filechooser]
            terminal = ""
        "#,
    )
    .unwrap();
    let empty = runtime::load_config(Some(tmp.path().to_path_buf()));
    assert_eq!(empty.unwrap_err(), "Terminal command cannot be empty");

    std::fs::write(
        tmp.path(),
        r#"
            [filechooser]
            terminal = "'unterminated"
        "#,
    )
    .unwrap();
    let malformed = runtime::load_config(Some(tmp.path().to_path_buf()));
    assert_eq!(
        malformed.unwrap_err(),
        "Terminal command has invalid quoting"
    );

    std::fs::write(
        tmp.path(),
        r#"
            [filechooser]
            chooser = ""
        "#,
    )
    .unwrap();
    let empty_chooser = runtime::load_config(Some(tmp.path().to_path_buf()));
    assert_eq!(
        empty_chooser.unwrap_err(),
        "Chooser command cannot be empty"
    );

    std::fs::write(
        tmp.path(),
        r#"
            [filechooser]
            default_dir = "relative"
        "#,
    )
    .unwrap();
    let relative_dir = runtime::load_config(Some(tmp.path().to_path_buf()));
    assert_eq!(
        relative_dir.unwrap_err(),
        "Default directory must be an absolute path"
    );
}

#[test]
fn save_cancellation_does_not_fabricate_current_file_selection() {
    let mut config = test_config();
    config.filechooser.terminal = "true".into();
    let chooser = FileChooser::new(config);
    let mut options = HashMap::new();
    options.insert(
        "current_file".into(),
        owned_value(Value::Array(
            b"/home/osso/Downloads/example.txt\0".to_vec().into(),
        )),
    );

    let (code, result) = chooser.save_file_result("Save", options);

    assert_eq!(code, 1);
    assert!(result.is_empty());
}
