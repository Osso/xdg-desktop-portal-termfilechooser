use super::*;
use std::future::Future;
#[cfg(not(test))]
use zbus::connection;
use zbus::{ObjectServer, interface};

pub(super) struct PortalRequest {
    cancellation: CancellationToken,
}

impl PortalRequest {
    pub(super) fn new(cancellation: CancellationToken) -> Self {
        Self { cancellation }
    }
}

#[interface(name = "org.freedesktop.impl.portal.Request")]
impl PortalRequest {
    pub(super) async fn close(&self) {
        self.cancellation.cancel();
    }
}

#[interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    async fn open_file(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
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
        run_registered_request(server, handle.to_owned(), |cancellation| {
            self.open_file_result_with_cancellation(title, options, cancellation)
        })
        .await
    }

    async fn save_file(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
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
        run_registered_request(server, handle.to_owned(), |cancellation| {
            self.save_file_result_with_cancellation(title, options, cancellation)
        })
        .await
    }

    async fn save_files(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: zbus::zvariant::ObjectPath<'_>,
        app_id: &str,
        _parent_window: &str,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        info!(
            "SaveFiles: handle={}, app_id={}, title={}",
            handle, app_id, title
        );
        debug!("Options: {:?}", options);
        run_registered_request(server, handle.to_owned(), |cancellation| {
            self.save_files_result_with_cancellation(title, options, cancellation)
        })
        .await
    }
}

async fn run_registered_request<F, Fut>(
    server: &ObjectServer,
    path: zbus::zvariant::ObjectPath<'static>,
    operation: F,
) -> (u32, HashMap<String, OwnedValue>)
where
    F: FnOnce(CancellationToken) -> Fut,
    Fut: Future<Output = (u32, HashMap<String, OwnedValue>)>,
{
    let Ok((request_path, cancellation)) = register_request(server, path).await else {
        return (2, HashMap::new());
    };
    let result = operation(cancellation).await;
    unregister_request(server, request_path).await;
    result
}

async fn register_request(
    server: &ObjectServer,
    path: zbus::zvariant::ObjectPath<'static>,
) -> Result<(zbus::zvariant::ObjectPath<'static>, CancellationToken), ()> {
    let cancellation = CancellationToken::new();
    match server
        .at(path.clone(), PortalRequest::new(cancellation.clone()))
        .await
    {
        Ok(true) => Ok((path, cancellation)),
        Ok(false) => {
            warn!("Request object already exists at {}", path);
            Err(())
        }
        Err(error) => {
            warn!("Failed to register request object at {}: {}", path, error);
            Err(())
        }
    }
}

async fn unregister_request(server: &ObjectServer, path: zbus::zvariant::ObjectPath<'static>) {
    if let Err(error) = server.remove::<PortalRequest, _>(&path).await {
        warn!("Failed to remove request object at {}: {}", path, error);
    }
}

pub(super) fn load_config(path: Option<PathBuf>) -> Result<Config, String> {
    let explicit_path = path.is_some();
    let config_path = path.or_else(default_config_path);

    let Some(path) = config_path else {
        let config = Config::default();
        config.validate()?;
        info!("Using default config");
        return Ok(config);
    };

    if !path.exists() {
        if explicit_path {
            return Err(format!("Config file does not exist: {}", path.display()));
        }
        let config = Config::default();
        config.validate()?;
        info!("Using default config");
        return Ok(config);
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read config {}: {error}", path.display()))?;
    let config: Config = toml::from_str(&content)
        .map_err(|error| format!("Failed to parse config {}: {error}", path.display()))?;
    config.validate()?;
    info!("Loaded config from {:?}", path);
    Ok(config)
}

pub(super) fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir()
        .map(|directory| directory.join("xdg-desktop-portal-termfilechooser/config.toml"))
}

#[cfg(all(not(test), not(coverage)))]
pub(super) fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&args.loglevel));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let config = load_config(args.config)?;
    debug!("Config: {:?}", config);

    let filechooser = FileChooser::new(config);
    zbus::block_on(async {
        let _connection = connection::Builder::session()?
            .allow_name_replacements(true)
            .replace_existing_names(args.replace)
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
