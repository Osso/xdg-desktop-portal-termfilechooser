use clap::Parser;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_encode};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};

/// Encode every byte except RFC 3986 unreserved characters and the path separator.
const PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'/');
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{debug, info, warn};
use zbus::zvariant::{OwnedValue, Value};

mod config;
mod portal_config;
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

    /// Select this backend for FileChooser in the active desktop portal policy
    #[arg(long)]
    configure_portal: bool,
}

fn get_bool_option(options: &HashMap<String, OwnedValue>, key: &str) -> bool {
    options
        .get(key)
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(false)
}

fn get_bytes_option(options: &HashMap<String, OwnedValue>, key: &str) -> Option<PathBuf> {
    options.get(key).and_then(|value| {
        let mut bytes = Vec::<u8>::try_from(value.clone()).ok()?;
        while bytes.last() == Some(&0) {
            bytes.pop();
        }
        if bytes.contains(&0) {
            return None;
        }
        Some(PathBuf::from(OsString::from_vec(bytes)))
    })
}

fn get_byte_arrays_option(
    options: &HashMap<String, OwnedValue>,
    key: &str,
) -> Option<Vec<OsString>> {
    let arrays = options
        .get(key)
        .and_then(|value| Vec::<Vec<u8>>::try_from(value.clone()).ok())?;
    arrays.into_iter().map(bytes_to_os_string).collect()
}

fn bytes_to_os_string(mut bytes: Vec<u8>) -> Option<OsString> {
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    Some(OsString::from_vec(bytes))
}

fn parent_dir_from_path(path: impl AsRef<Path>) -> Option<PathBuf> {
    path.as_ref().parent().map(Path::to_path_buf)
}

#[derive(Clone, Default)]
struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    fn new() -> Self {
        Self::default()
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

struct RunningTerminal {
    result: async_channel::Receiver<Result<(), String>>,
}

impl RunningTerminal {
    async fn wait(self) -> Result<(), String> {
        self.result
            .recv()
            .await
            .map_err(|_| "Terminal worker stopped without a result".to_string())?
    }
}

struct FileChooser {
    config: Config,
}

#[derive(Clone, Copy)]
struct ChooserRequest<'a> {
    title: &'a str,
    start_path: Option<&'a Path>,
    multiple: bool,
}

impl FileChooser {
    fn new(config: Config) -> Self {
        Self { config }
    }

    fn build_chooser_args(&self, out_path: &str, start: &Path) -> Vec<OsString> {
        let mut args = parse_command(&self.config.filechooser.chooser, "Chooser")
            .expect("validated chooser command");
        let last = args.pop().unwrap_or_default();
        args.push(OsString::from(format!(
            "{}={}",
            last.to_string_lossy(),
            out_path
        )));
        args.push(start.as_os_str().to_os_string());
        args
    }

    fn start_terminal(
        &self,
        title: &str,
        chooser_args: &[OsString],
        cancellation: CancellationToken,
    ) -> Result<RunningTerminal, String> {
        let mut command = self.build_terminal_command(title, chooser_args)?;
        debug!("Running: {:?}", command);
        let child = command
            .spawn()
            .map_err(|error| format!("Failed to spawn terminal: {error}"))?;
        Ok(spawn_terminal_worker(child, cancellation))
    }

    fn build_terminal_command(
        &self,
        title: &str,
        chooser_args: &[OsString],
    ) -> Result<Command, String> {
        let mut term_parts = parse_command(&self.config.filechooser.terminal, "Terminal")?;
        let term_cmd = term_parts.remove(0);
        let mut command = Command::new(term_cmd);
        command
            .process_group(0)
            .args(term_parts)
            .arg(title)
            .arg("--")
            .arg(&chooser_args[0])
            .args(&chooser_args[1..]);
        Ok(command)
    }

    #[cfg(test)]
    fn spawn_terminal(&self, title: &str, chooser_args: &[OsString]) -> Result<(), String> {
        let running = self.start_terminal(title, chooser_args, CancellationToken::new())?;
        zbus::block_on(running.wait())
    }

    fn read_selections(&self, out_path: &str) -> Vec<PathBuf> {
        let content = std::fs::read(out_path).unwrap_or_default();

        content
            .split(|byte| *byte == b'\n')
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
            .filter(|line| !line.is_empty())
            .map(|line| PathBuf::from(OsString::from_vec(line.to_vec())))
            .collect()
    }

    async fn run_chooser_paths_with_cancellation(
        &self,
        request: ChooserRequest<'_>,
        cancellation: CancellationToken,
    ) -> Result<Vec<PathBuf>, String> {
        let output = tempfile::NamedTempFile::new()
            .map_err(|error| format!("Failed to create temp file: {error}"))?;
        let out_path = output.path().to_string_lossy().to_string();
        let start = request
            .start_path
            .unwrap_or_else(|| Path::new(&self.config.filechooser.default_dir));
        let chooser_args = self.build_chooser_args(&out_path, start);
        self.start_terminal(request.title, &chooser_args, cancellation)?
            .wait()
            .await?;
        let selections = self.read_selections(&out_path);
        validate_selections(&selections, request.multiple)?;
        Ok(selections)
    }

    async fn run_chooser_with_cancellation(
        &self,
        request: ChooserRequest<'_>,
        cancellation: CancellationToken,
    ) -> Result<Vec<String>, String> {
        self.run_chooser_paths_with_cancellation(request, cancellation)
            .await?
            .iter()
            .map(|path| path_to_file_uri(path))
            .collect()
    }

    #[cfg(test)]
    fn run_chooser(&self, request: ChooserRequest<'_>) -> Result<Vec<String>, String> {
        zbus::block_on(self.run_chooser_with_cancellation(request, CancellationToken::new()))
    }

    async fn open_file_result_with_cancellation(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
        cancellation: CancellationToken,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let current_folder = get_bytes_option(&options, "current_folder");
        let request = ChooserRequest {
            title,
            start_path: current_folder.as_deref(),
            multiple: get_bool_option(&options, "multiple"),
        };
        match self
            .run_chooser_with_cancellation(request, cancellation)
            .await
        {
            Ok(uris) => {
                info!("Selected {} file(s)", uris.len());
                (0, build_uris_result(uris))
            }
            Err(error) => {
                warn!("OpenFile cancelled or failed: {}", error);
                (response_code_for_error(&error), HashMap::new())
            }
        }
    }

    #[cfg(test)]
    fn open_file_result(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        zbus::block_on(self.open_file_result_with_cancellation(
            title,
            options,
            CancellationToken::new(),
        ))
    }

    async fn save_files_result_with_cancellation(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
        cancellation: CancellationToken,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let Some(file_names) = get_byte_arrays_option(&options, "files") else {
            warn!("SaveFiles failed: missing or invalid files option");
            return (2, HashMap::new());
        };
        if file_names.iter().any(|name| !is_safe_file_name(name)) {
            warn!("SaveFiles failed: files must contain plain file names");
            return (2, HashMap::new());
        }
        let current_folder = get_bytes_option(&options, "current_folder");
        let request = ChooserRequest {
            title,
            start_path: current_folder.as_deref(),
            multiple: false,
        };
        let selected = self
            .run_chooser_paths_with_cancellation(request, cancellation)
            .await;
        match selected {
            Ok(paths) => save_files_success_result(&paths[0], &file_names),
            Err(error) => {
                warn!("SaveFiles cancelled or failed: {}", error);
                (response_code_for_error(&error), HashMap::new())
            }
        }
    }

    #[cfg(test)]
    fn save_files_result(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        zbus::block_on(self.save_files_result_with_cancellation(
            title,
            options,
            CancellationToken::new(),
        ))
    }

    async fn save_file_result_with_cancellation(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
        cancellation: CancellationToken,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let current_folder = get_bytes_option(&options, "current_folder");
        let current_file = get_bytes_option(&options, "current_file");
        let derived_start = current_file.as_deref().and_then(parent_dir_from_path);
        let request = ChooserRequest {
            title,
            start_path: current_folder.as_deref().or(derived_start.as_deref()),
            multiple: false,
        };
        match self
            .run_chooser_with_cancellation(request, cancellation)
            .await
        {
            Ok(uris) => {
                info!("Save location: {:?}", uris);
                (0, build_uris_result(uris))
            }
            Err(error) => {
                warn!("SaveFile cancelled or failed: {}", error);
                (response_code_for_error(&error), HashMap::new())
            }
        }
    }

    #[cfg(test)]
    fn save_file_result(
        &self,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        zbus::block_on(self.save_file_result_with_cancellation(
            title,
            options,
            CancellationToken::new(),
        ))
    }
}

fn spawn_terminal_worker(child: Child, cancellation: CancellationToken) -> RunningTerminal {
    let (sender, result) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let outcome = wait_for_terminal(child, cancellation);
        if sender.send_blocking(outcome).is_err() {
            debug!("Terminal result receiver was dropped");
        }
    });
    RunningTerminal { result }
}

fn wait_for_terminal(mut child: Child, cancellation: CancellationToken) -> Result<(), String> {
    loop {
        if cancellation.is_cancelled() {
            let process_group = rustix::process::Pid::from_child(&child);
            if let Err(error) =
                rustix::process::kill_process_group(process_group, rustix::process::Signal::KILL)
            {
                debug!("Terminal process group was already stopped: {}", error);
            }
            if let Err(error) = child.wait() {
                warn!("Failed to reap cancelled terminal process: {}", error);
            }
            return Err("Request cancelled".into());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("Terminal exited with: {status}")),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(format!("Failed to wait for terminal: {error}")),
        }
    }
}

fn validate_selections(selections: &[PathBuf], multiple: bool) -> Result<(), String> {
    if selections.is_empty() {
        return Err("No files selected".into());
    }
    if !multiple && selections.len() != 1 {
        return Err(format!(
            "Chooser returned {} paths for a single-selection request",
            selections.len()
        ));
    }
    if let Some(relative) = selections.iter().find(|path| !path.is_absolute()) {
        return Err(format!(
            "Chooser returned a relative path: {}",
            relative.to_string_lossy()
        ));
    }
    Ok(())
}

fn save_files_success_result(
    directory: &Path,
    file_names: &[OsString],
) -> (u32, HashMap<String, OwnedValue>) {
    if !directory.is_dir() {
        warn!("SaveFiles failed: selected path is not a directory");
        return (2, HashMap::new());
    }
    let uris = file_names
        .iter()
        .map(|name| path_to_file_uri(&directory.join(name)))
        .collect::<Result<Vec<_>, _>>();
    match uris {
        Ok(uris) => (0, build_uris_result(uris)),
        Err(error) => {
            warn!("SaveFiles failed: {}", error);
            (2, HashMap::new())
        }
    }
}

fn is_safe_file_name(name: &OsStr) -> bool {
    let path = Path::new(name);
    let mut components = path.components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn path_to_file_uri(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err(format!(
            "Chooser returned a relative path: {}",
            path.to_string_lossy()
        ));
    }
    Ok(format!(
        "file://{}",
        percent_encode(path.as_os_str().as_bytes(), PATH_ENCODE_SET)
    ))
}

fn response_code_for_error(error: &str) -> u32 {
    if matches!(error, "No files selected" | "Request cancelled") {
        1
    } else {
        2
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
mod tests;

#[cfg(all(not(test), not(coverage)))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.configure_portal {
        let path = portal_config::configure()?;
        println!("Updated portal policy: {}", path.display());
        return Ok(());
    }
    runtime::run(args)
}
