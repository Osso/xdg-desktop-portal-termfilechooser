use std::ffi::OsString;
use std::path::Path;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub(super) struct Config {
    #[serde(default)]
    pub(super) filechooser: FileChooserConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(super) struct FileChooserConfig {
    #[serde(default = "default_terminal")]
    pub(super) terminal: String,
    #[serde(default = "default_chooser")]
    pub(super) chooser: String,
    #[serde(default = "default_dir")]
    pub(super) default_dir: String,
}

pub(super) fn default_terminal() -> String {
    "kitty --class file-chooser --title".into()
}

pub(super) fn default_chooser() -> String {
    "yazi --chooser-file".into()
}

fn default_dir() -> String {
    match dirs::home_dir() {
        Some(path) => path.to_string_lossy().into_owned(),
        None => "/".into(),
    }
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

impl Config {
    pub(super) fn validate(&self) -> Result<(), String> {
        parse_command(&self.filechooser.terminal, "Terminal")?;
        parse_command(&self.filechooser.chooser, "Chooser")?;
        if !Path::new(&self.filechooser.default_dir).is_absolute() {
            return Err("Default directory must be an absolute path".into());
        }
        Ok(())
    }
}

pub(super) fn parse_command(command: &str, label: &str) -> Result<Vec<OsString>, String> {
    let parts =
        shlex::split(command).ok_or_else(|| format!("{label} command has invalid quoting"))?;
    if parts.is_empty() {
        return Err(format!("{label} command cannot be empty"));
    }
    Ok(parts.into_iter().map(OsString::from).collect())
}
