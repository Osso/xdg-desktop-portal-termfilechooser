use clap::Parser;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
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
use zbus::object_server::SignalEmitter;
use zbus::{connection, interface};
use zbus::zvariant::{OwnedValue, Value};

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

#[derive(Debug, Clone, serde::Deserialize)]
struct Config {
    #[serde(default)]
    filechooser: FileChooserConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FileChooserConfig {
    #[serde(default = "default_terminal")]
    terminal: String,
    #[serde(default = "default_chooser")]
    chooser: String,
    #[serde(default = "default_dir")]
    default_dir: String,
}

fn default_terminal() -> String {
    "kitty --class file-chooser --title".into()
}

fn default_chooser() -> String {
    "yazi --chooser-file".into()
}

fn default_dir() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".into())
}

impl Default for FileChooserConfig {
    fn default() -> Self {
        Self {
            terminal: default_terminal(),
            chooser: default_chooser(),
            default_dir: default_dir(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            filechooser: FileChooserConfig::default(),
        }
    }
}

fn get_bool_option(options: &HashMap<String, OwnedValue>, key: &str) -> bool {
    options.get(key).and_then(|v| {
        bool::try_from(v.clone()).ok()
    }).unwrap_or(false)
}

fn get_bytes_option(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    options.get(key).and_then(|v| {
        <Vec<u8>>::try_from(v.clone())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|s| s.trim_end_matches('\0').to_string())
    })
}

struct FileChooser {
    config: Config,
}

impl FileChooser {
    fn new(config: Config) -> Self {
        Self { config }
    }

    fn run_chooser(
        &self,
        title: &str,
        start_path: Option<&str>,
        _save: bool,
        directory: bool,
        _multiple: bool,
    ) -> Result<Vec<String>, String> {
        let tmp = tempfile::NamedTempFile::new()
            .map_err(|e| format!("Failed to create temp file: {}", e))?;
        let out_path = tmp.path().to_string_lossy().to_string();

        let start = start_path.unwrap_or(&self.config.filechooser.default_dir);

        // Build chooser command: "yazi --chooser-file" -> ["yazi", "--chooser-file=/tmp/xxx"]
        let mut chooser_args: Vec<String> = self
            .config
            .filechooser
            .chooser
            .split_whitespace()
            .map(String::from)
            .collect();

        let last_arg = chooser_args.pop().unwrap_or_default();
        chooser_args.push(format!("{}={}", last_arg, out_path));

        if directory {
            chooser_args.push(format!("--cwd-file={}.dir", out_path));
        }
        chooser_args.push(start.to_string());

        // Build terminal command
        let mut term_parts: Vec<&str> = self.config.filechooser.terminal.split_whitespace().collect();
        let term_cmd = term_parts.remove(0);

        let mut cmd = Command::new(term_cmd);
        cmd.args(&term_parts);
        cmd.arg(title);
        cmd.arg("--");
        cmd.arg(&chooser_args[0]);
        cmd.args(&chooser_args[1..]);

        debug!("Running: {:?}", cmd);

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to spawn terminal: {}", e))?;

        if !status.success() {
            return Err(format!("Terminal exited with: {}", status));
        }

        // Read selections
        let content = std::fs::read_to_string(&out_path).unwrap_or_default();

        // Handle directory selection fallback
        let dir_path = format!("{}.dir", out_path);
        let dir_content = std::fs::read_to_string(&dir_path).ok();
        let _ = std::fs::remove_file(&dir_path);

        let selections: Vec<String> = if content.trim().is_empty() {
            if let Some(dc) = dir_content {
                dc.lines()
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect()
            } else {
                vec![]
            }
        } else {
            content
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        };

        if selections.is_empty() {
            return Err("No files selected".into());
        }

        // Convert to file:// URIs
        let uris: Vec<String> = selections
            .into_iter()
            .map(|path| {
                let encoded = utf8_percent_encode(&path, PATH_ENCODE_SET).to_string();
                format!("file://{}", encoded)
            })
            .collect();

        Ok(uris)
    }
}

fn build_uris_result(uris: Vec<String>) -> HashMap<String, OwnedValue> {
    let mut results = HashMap::new();
    // Must be array of strings (as), not array of variants (av)
    let array: zbus::zvariant::Array = uris.into();
    results.insert("uris".to_string(), Value::Array(array).try_into().unwrap());
    results
}

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    async fn open_file(
        &self,
        #[zbus(signal_emitter)] _emitter: SignalEmitter<'_>,
        handle: zbus::zvariant::ObjectPath<'_>,
        app_id: &str,
        _parent_window: &str,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        info!("OpenFile: handle={}, app_id={}, title={}", handle, app_id, title);
        debug!("Options: {:?}", options);

        let multiple = get_bool_option(&options, "multiple");
        let directory = get_bool_option(&options, "directory");
        let current_folder = get_bytes_option(&options, "current_folder");

        match self.run_chooser(title, current_folder.as_deref(), false, directory, multiple) {
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

    async fn save_file(
        &self,
        #[zbus(signal_emitter)] _emitter: SignalEmitter<'_>,
        handle: zbus::zvariant::ObjectPath<'_>,
        app_id: &str,
        _parent_window: &str,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        info!("SaveFile: handle={}, app_id={}, title={}", handle, app_id, title);
        debug!("Options: {:?}", options);

        let current_folder = get_bytes_option(&options, "current_folder");

        match self.run_chooser(title, current_folder.as_deref(), true, false, false) {
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

fn load_config(path: Option<PathBuf>) -> Config {
    let config_path = path.or_else(|| {
        dirs::config_dir().map(|d| d.join("xdg-desktop-portal-termfilechooser/config.toml"))
    });

    if let Some(path) = config_path {
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(config) => {
                        info!("Loaded config from {:?}", path);
                        return config;
                    }
                    Err(e) => warn!("Failed to parse config: {}", e),
                },
                Err(e) => warn!("Failed to read config: {}", e),
            }
        }
    }

    info!("Using default config");
    Config::default()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Set up logging
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&args.loglevel));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let config = load_config(args.config);
    debug!("Config: {:?}", config);

    let filechooser = FileChooser::new(config);

    // Use zbus's built-in async runtime
    zbus::block_on(async {
        let _conn = connection::Builder::session()?
            .name("org.freedesktop.impl.portal.desktop.termfilechooser")?
            .serve_at("/org/freedesktop/portal/desktop", filechooser)?
            .build()
            .await?;

        info!("Service registered on D-Bus");

        // Wait forever
        std::future::pending::<()>().await;
        Ok::<(), zbus::Error>(())
    })?;

    Ok(())
}
