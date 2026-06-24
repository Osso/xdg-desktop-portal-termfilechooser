use super::*;
#[cfg(not(test))]
use zbus::object_server::SignalEmitter;
#[cfg(not(test))]
use zbus::{connection, interface};

#[cfg(not(test))]
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
        info!(
            "OpenFile: handle={}, app_id={}, title={}",
            handle, app_id, title
        );
        debug!("Options: {:?}", options);
        self.open_file_result(title, options)
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
        info!(
            "SaveFile: handle={}, app_id={}, title={}",
            handle, app_id, title
        );
        debug!("Options: {:?}", options);
        self.save_file_result(title, options)
    }
}

pub(super) fn load_config(path: Option<PathBuf>) -> Config {
    let config_path = path.or_else(|| {
        dirs::config_dir().map(|d| d.join("xdg-desktop-portal-termfilechooser/config.toml"))
    });

    let Some(path) = config_path else {
        info!("Using default config");
        return Config::default();
    };

    if !path.exists() {
        info!("Using default config");
        return Config::default();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) => {
            warn!("Failed to read config: {}", e);
            info!("Using default config");
            return Config::default();
        }
    };

    match toml::from_str(&content) {
        Ok(config) => {
            info!("Loaded config from {:?}", path);
            config
        }
        Err(e) => {
            warn!("Failed to parse config: {}", e);
            info!("Using default config");
            Config::default()
        }
    }
}

#[cfg(not(test))]
pub(super) fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&args.loglevel));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let config = load_config(args.config);
    debug!("Config: {:?}", config);

    let filechooser = FileChooser::new(config);
    zbus::block_on(async {
        let _conn = connection::Builder::session()?
            .name("org.freedesktop.impl.portal.desktop.termfilechooser")?
            .serve_at("/org/freedesktop/portal/desktop", filechooser)?
            .build()
            .await?;

        info!("Service registered on D-Bus");
        std::future::pending::<()>().await;
        Ok::<(), zbus::Error>(())
    })?;

    Ok(())
}
