#[derive(Debug, Clone, serde::Deserialize)]
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

impl Default for Config {
    fn default() -> Self {
        Self {
            filechooser: FileChooserConfig::default(),
        }
    }
}
