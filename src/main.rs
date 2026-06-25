use clap::Parser;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use std::collections::HashMap;

/// Characters to encode in file URIs (everything except unreserved + /)
const PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info, warn};
use zbus::zvariant::{OwnedValue, Value};

mod config;
mod runtime;
use config::*;

#[derive(Parser)]
#[command(name = "xdg-desktop-portal-termfilechooser")]
#[command(about = "XDG Desktop Portal backend for terminal file choosers")]
struct Args {
    /// Replace a running instance
    #[arg(short, long)]
    replace: bool,

    /// Log level (error, warn, info, debug, trace)
    #[arg(short, long, default_value = "info")]
    loglevel: String,

    /// Config file path
    #[arg(short, long)]
    config: Option<PathBuf>,
}

fn get_bool_option(options: &HashMap<String, OwnedValue>, key: &str) -> bool {
    options
        .get(key)
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(false)
}

fn get_bytes_option(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    options.get(key).and_then(|v| {
        <Vec<u8>>::try_from(v.clone())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|s| s.trim_end_matches('\0').to_string())
    })
}

fn get_string_option(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(|v| String::try_from(v.clone()).ok())
}

fn file_name_from_path(path: &str) -> Option<String> {
    PathBuf::from(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn parent_dir_from_path(path: &str) -> Option<String> {
    PathBuf::from(path)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
}

struct FileChooser {
    config: Config,
}

impl FileChooser {
    fn new(config: Config) -> Self {
        Self { config }
    }

    fn build_chooser_args(
        &self,
        out_path: &str,
        directory: bool,
        save: bool,
        start: &str,
    ) -> Vec<String> {
        let mut args: Vec<String> = self
            .config
            .filechooser
            .chooser
            .split_whitespace()
            .map(String::from)
            .collect();
        let last = args.pop().unwrap_or_default();
        args.push(format!("{}={}", last, out_path));
        if directory || save {
            args.push(format!("--cwd-file={}.dir", out_path));
        }
        args.push(start.to_string());
        args
    }

    fn spawn_terminal(&self, title: &str, chooser_args: &[String]) -> Result<(), String> {
        let mut term_parts: Vec<String> = self
            .config
            .filechooser
            .terminal
            .split_whitespace()
            .map(String::from)
            .collect();
        let term_cmd = term_parts.remove(0);
        let title = title.to_string();
        let chooser_args = chooser_args.to_vec();

        // Run in a separate thread to avoid tokio reactor panics from zbus's async context
        std::thread::spawn(move || {
            let mut cmd = Command::new(&term_cmd);
            cmd.args(&term_parts)
                .arg(&title)
                .arg("--")
                .arg(&chooser_args[0])
                .args(&chooser_args[1..]);
            debug!("Running: {:?}", cmd);
            let status = cmd
                .status()
                .map_err(|e| format!("Failed to spawn terminal: {}", e))?;
            if !status.success() {
                return Err(format!("Terminal exited with: {}", status));
            }
            Ok(())
        })
        .join()
        .map_err(|_| "Terminal thread panicked".to_string())?
    }

    fn read_selections(
        &self,
        out_path: &str,
        save: bool,
        suggested_name: Option<&str>,
        current_file: Option<&str>,
        start: &str,
    ) -> Vec<String> {
        let content = std::fs::read_to_string(out_path).unwrap_or_default();
        let dir_path = format!("{}.dir", out_path);
        let dir_content = std::fs::read_to_string(&dir_path).ok();
        let _ = std::fs::remove_file(&dir_path);
        if !content.trim().is_empty() {
            return content
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect();
        }
        if save {
            let dir = dir_content
                .as_deref()
                .and_then(|dc| dc.lines().next())
                .map(String::from)
                .unwrap_or_else(|| start.to_string());

            let name = suggested_name
                .map(String::from)
                .or_else(|| current_file.and_then(file_name_from_path));

            if let Some(name) = name {
                return vec![PathBuf::from(dir).join(name).to_string_lossy().into_owned()];
            }
        }
        dir_content
            .map(|dc| {
                dc.lines()
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn run_chooser(
        &self,
        title: &str,
        start_path: Option<&str>,
        save: bool,
        directory: bool,
        _multiple: bool,
        suggested_name: Option<&str>,
        current_file: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let tmp = tempfile::NamedTempFile::new()
            .map_err(|e| format!("Failed to create temp file: {}", e))?;
        let out_path = tmp.path().to_string_lossy().to_string();
        let derived_start = current_file.and_then(parent_dir_from_path);
        let start = start_path
            .or(derived_start.as_deref())
            .unwrap_or(&self.config.filechooser.default_dir);
        let chooser_args = self.build_chooser_args(&out_path, directory, save, start);
        self.spawn_terminal(title, &chooser_args)?;
        let selections = self.read_selections(&out_path, save, suggested_name, current_file, start);
        if selections.is_empty() {
            return Err("No files selected".into());
        }
        let uris = selections
            .into_iter()
            .map(|path| format!("file://{}", utf8_percent_encode(&path, PATH_ENCODE_SET)))
            .collect();
        Ok(uris)
    }

    fn open_file_result(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let multiple = get_bool_option(&options, "multiple");
        let directory = get_bool_option(&options, "directory");
        let current_folder = get_bytes_option(&options, "current_folder");

        match self.run_chooser(
            title,
            current_folder.as_deref(),
            false,
            directory,
            multiple,
            None,
            None,
        ) {
            Ok(uris) => {
                info!("Selected {} file(s)", uris.len());
                (0, build_uris_result(uris))
            }
            Err(e) => {
                warn!("OpenFile cancelled or failed: {}", e);
                (1, HashMap::new())
            }
        }
    }

    fn save_file_result(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let current_folder = get_bytes_option(&options, "current_folder");
        let current_name = get_string_option(&options, "current_name");
        let current_file = get_bytes_option(&options, "current_file");

        match self.run_chooser(
            title,
            current_folder.as_deref(),
            true,
            false,
            false,
            current_name.as_deref(),
            current_file.as_deref(),
        ) {
            Ok(uris) => {
                info!("Save location: {:?}", uris);
                (0, build_uris_result(uris))
            }
            Err(e) => {
                warn!("SaveFile cancelled or failed: {}", e);
                (1, HashMap::new())
            }
        }
    }
}

fn build_uris_result(uris: Vec<String>) -> HashMap<String, OwnedValue> {
    let mut results = HashMap::new();
    // Must be array of strings (as), not array of variants (av)
    let array: zbus::zvariant::Array = uris.into();
    results.insert("uris".to_string(), Value::Array(array).try_into().unwrap());
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

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
            "current_name".into(),
            owned_value(Value::Str("report.txt".into())),
        );
        options.insert(
            "current_folder".into(),
            owned_value(Value::Array(vec![b'/', b't', b'm', b'p', b'\0'].into())),
        );

        assert!(get_bool_option(&options, "multiple"));
        assert!(!get_bool_option(&options, "missing"));
        assert_eq!(
            get_string_option(&options, "current_name"),
            Some("report.txt".into())
        );
        assert_eq!(
            get_bytes_option(&options, "current_folder"),
            Some("/tmp".into())
        );

        options.insert("not_bool".into(), owned_value(Value::Str("true".into())));
        options.insert("not_string".into(), owned_value(Value::Bool(true)));
        options.insert("not_bytes".into(), owned_value(Value::Str("/tmp".into())));
        assert!(!get_bool_option(&options, "not_bool"));
        assert_eq!(get_string_option(&options, "not_string"), None);
        assert_eq!(get_bytes_option(&options, "not_bytes"), None);
    }

    #[test]
    fn path_helpers_extract_basename_and_parent() {
        assert_eq!(
            file_name_from_path("/home/osso/Downloads/example.txt"),
            Some("example.txt".into())
        );
        assert_eq!(
            parent_dir_from_path("/home/osso/Downloads/example.txt"),
            Some("/home/osso/Downloads".into())
        );
        assert_eq!(file_name_from_path("/"), None);
    }

    #[test]
    fn chooser_args_insert_output_file_and_directory_sidecar() {
        let chooser = FileChooser::new(test_config());

        assert_eq!(
            chooser.build_chooser_args("/tmp/selection", false, false, "/home/osso"),
            vec!["yazi", "--chooser-file=/tmp/selection", "/home/osso"]
        );
        assert_eq!(
            chooser.build_chooser_args("/tmp/selection", true, false, "/home/osso"),
            vec![
                "yazi",
                "--chooser-file=/tmp/selection",
                "--cwd-file=/tmp/selection.dir",
                "/home/osso"
            ]
        );
        assert_eq!(
            chooser.build_chooser_args("/tmp/selection", false, true, "/home/osso"),
            vec![
                "yazi",
                "--chooser-file=/tmp/selection",
                "--cwd-file=/tmp/selection.dir",
                "/home/osso"
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
    fn read_selections_prefers_explicit_output_paths() {
        let chooser = FileChooser::new(test_config());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "/tmp/a.txt\n\n/tmp/b.txt\n").unwrap();

        let selections = chooser.read_selections(
            &tmp.path().to_string_lossy(),
            false,
            None,
            None,
            "/home/osso",
        );

        assert_eq!(selections, vec!["/tmp/a.txt", "/tmp/b.txt"]);
    }

    #[test]
    fn read_selections_can_return_selected_directories() {
        let chooser = FileChooser::new(test_config());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let dir_path = format!("{}.dir", tmp.path().display());
        std::fs::write(&dir_path, "/home/osso/Documents\n/home/osso/Pictures\n").unwrap();

        let selections = chooser.read_selections(
            &tmp.path().to_string_lossy(),
            false,
            None,
            None,
            "/home/osso",
        );

        assert_eq!(
            selections,
            vec!["/home/osso/Documents", "/home/osso/Pictures"]
        );
        assert!(!std::path::Path::new(&dir_path).exists());
    }

    #[test]
    fn save_uses_suggested_name_before_current_file_basename() {
        let chooser = FileChooser::new(test_config());
        let tmp = tempfile::NamedTempFile::new().unwrap();

        let selections = chooser.read_selections(
            &tmp.path().to_string_lossy(),
            true,
            Some("renamed.txt"),
            Some("/home/osso/Downloads/example.txt"),
            "/home/osso/Downloads",
        );

        assert_eq!(selections, vec!["/home/osso/Downloads/renamed.txt"]);
    }

    #[test]
    fn run_chooser_returns_error_when_terminal_writes_no_selection() {
        let mut config = test_config();
        config.filechooser.terminal = "true".into();
        let chooser = FileChooser::new(config);

        let error = chooser
            .run_chooser(
                "ignored",
                Some("/home/osso/Downloads"),
                false,
                false,
                false,
                None,
                None,
            )
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
            .run_chooser("Open", None, false, false, false, None, None)
            .unwrap();

        assert_eq!(selections, vec!["file:///tmp/space%20name%20%231.txt"]);
    }

    #[test]
    fn run_chooser_derives_start_from_current_file_when_folder_missing() {
        let terminal = executable_script(
            "terminal",
            r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in
        --cwd-file=*) cwd="${arg#--cwd-file=}" ;;
    esac
done
printf '/home/osso/Documents\n' > "$cwd"
"#,
        );
        let mut config = test_config();
        config.filechooser.terminal = terminal.to_string_lossy().into_owned();
        config.filechooser.chooser = "chooser --chooser-file".into();
        let chooser = FileChooser::new(config);

        let selections = chooser
            .run_chooser(
                "Save",
                None,
                true,
                false,
                false,
                None,
                Some("/home/osso/Downloads/report.txt"),
            )
            .unwrap();

        assert_eq!(selections, vec!["file:///home/osso/Documents/report.txt"]);
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
    }

    #[test]
    fn save_file_result_uses_portal_options_or_cancel_code() {
        let terminal = executable_script(
            "terminal",
            r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in
        --cwd-file=*) cwd="${arg#--cwd-file=}" ;;
    esac
done
printf '/tmp\n' > "$cwd"
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

        let config = runtime::load_config(Some(tmp.path().to_path_buf()));

        assert_eq!(config.filechooser.terminal, "foot --title");
        assert_eq!(config.filechooser.chooser, "ranger --choosefile");
        assert_eq!(config.filechooser.default_dir, "/tmp");

        let missing = runtime::load_config(Some(tmp.path().with_extension("missing")));
        assert_eq!(missing.filechooser.chooser, default_chooser());

        std::fs::write(tmp.path(), "[filechooser").unwrap();
        let invalid = runtime::load_config(Some(tmp.path().to_path_buf()));
        assert_eq!(invalid.filechooser.chooser, default_chooser());

        let read_error = runtime::load_config(Some(std::env::temp_dir()));
        assert_eq!(read_error.filechooser.chooser, default_chooser());
    }

    #[test]
    fn save_falls_back_to_current_file_when_no_new_selection_is_written() {
        let chooser = FileChooser::new(test_config());
        let tmp = tempfile::NamedTempFile::new().unwrap();

        let selections = chooser.read_selections(
            &tmp.path().to_string_lossy(),
            true,
            None,
            Some("/home/osso/Downloads/example.txt"),
            "/home/osso/Downloads",
        );

        assert_eq!(selections, vec!["/home/osso/Downloads/example.txt"]);
    }

    #[test]
    fn save_uses_selected_directory_with_current_file_basename() {
        let chooser = FileChooser::new(test_config());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let dir_path = format!("{}.dir", tmp.path().display());
        std::fs::write(&dir_path, "/home/osso/Documents\n").unwrap();

        let selections = chooser.read_selections(
            &tmp.path().to_string_lossy(),
            true,
            None,
            Some("/home/osso/Downloads/example.txt"),
            "/home/osso/Downloads",
        );

        assert_eq!(selections, vec!["/home/osso/Documents/example.txt"]);
    }

    #[test]
    fn save_without_name_or_current_file_returns_selected_directory() {
        let chooser = FileChooser::new(test_config());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let dir_path = format!("{}.dir", tmp.path().display());
        std::fs::write(&dir_path, "/home/osso/Documents\n").unwrap();

        let selections =
            chooser.read_selections(&tmp.path().to_string_lossy(), true, None, None, "/tmp");

        assert_eq!(selections, vec!["/home/osso/Documents"]);
    }
}

#[cfg(all(not(test), not(coverage)))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    runtime::run(Args::parse())
}
