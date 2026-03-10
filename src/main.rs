use std::sync::Arc;
use tracing::info;

mod api;
mod config;
mod errors;
mod metrics;
mod services;

use config::{OperationMode, UnifiedConfig, cli::parse_cli};
use errors::{ProxyError, ProxyResult};
use services::ServiceManager;

fn main() -> ProxyResult<()> {
    // Load .env first so RUST_LOG is in the environment before anything reads it.
    dotenvy::dotenv().ok();

    let cli = parse_cli();

    // Config is loaded before tracing so we can use the log level it specifies.
    // Any errors here go to stderr via eprintln since the subscriber isn't up yet.
    let mut config = UnifiedConfig::load_from_file(&cli.config).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    // Initialise tracing.
    // Priority: RUST_LOG env var  >  config.yaml logging.level  >  "info"
    {
        use tracing_subscriber::EnvFilter;
        let config_level = config
            .logging
            .as_ref()
            .map(|l| l.level.as_str())
            .unwrap_or("info");
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(config_level));
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    // CLI overrides
    if let Some(mode_str) = &cli.mode {
        config.mode = match mode_str.as_str() {
            "lb" => OperationMode::LoadBalancer,
            "gateway" => OperationMode::ApiGateway,
            other => {
                eprintln!("error: unknown mode '{other}' — use lb | gateway");
                std::process::exit(1);
            }
        };
    }
    if let Some(bind) = &cli.bind {
        config.server.bind_address = bind.clone();
    }

    info!(
        mode = ?config.mode,
        bind = %config.server.bind_address,
        "Starting Locci Proxy"
    );

    // Force-initialise all metric descriptors so they appear in scrape output
    // from startup, before any requests arrive.
    metrics::init();

    // Pingora owns its runtime; plain fn main() + run_forever() is required.
    let mut server =
        pingora_core::server::Server::new(None).map_err(|e| ProxyError::LoadBalancerBuild {
            name: "pingora-server".to_owned(),
            source: std::io::Error::other(e.to_string()),
        })?;
    server.bootstrap();

    let manager = Arc::new(ServiceManager::new(config.clone()));

    match config.mode {
        OperationMode::LoadBalancer => services::lb::add_lb_service(&mut server, &config)?,
        OperationMode::ApiGateway => services::gateway::add_gateway_service(&mut server, &config)?,
    }

    // Control API runs on its own tokio runtime in a background thread.
    if config.control_api.enabled {
        let control_cfg = config.control_api.clone();
        let mgr = manager.clone();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(api::start_control_api(mgr, control_cfg));
        });
    }

    server.run_forever();
}
