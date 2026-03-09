use thiserror::Error;

/// Project-wide result type alias.
pub type ProxyResult<T> = Result<T, ProxyError>;

/// All errors that can be produced during configuration loading and service initialisation.
/// Request-time errors inside Pingora proxy trait methods use `pingora_core::Error` directly
/// (the hot path) but may call `.to_string()` on these variants to populate error messages.
#[derive(Debug, Error)]
pub enum ProxyError {
    // ── Configuration ────────────────────────────────────────────────────────
    #[error("could not read config file '{path}': {source}")]
    ConfigFileRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not parse config file '{path}': {source}")]
    ConfigFileParse {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("mode '{mode}' requires a '{section}' section in the config file")]
    MissingConfigSection { mode: String, section: &'static str },

    // ── Service initialisation ────────────────────────────────────────────────
    #[error("invalid regex pattern '{pattern}': {source}")]
    InvalidRegex {
        pattern: String,
        #[source]
        source: regex::Error,
    },

    #[error("upstream group '{name}' has no servers configured")]
    EmptyUpstream { name: String },

    #[error("could not build load balancer for upstream '{name}': {source}")]
    LoadBalancerBuild {
        name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid health-check URI '{uri}': {reason}")]
    InvalidHealthCheckUri { uri: String, reason: String },

    // ── Request-time (used for message text in pingora Error::new_str) ────────
    #[error("upstream group '{name}' not found")]
    UpstreamNotFound { name: String },

    #[error("upstream group '{name}' has no healthy peers")]
    NoHealthyPeers { name: String },

    #[error("no route matched path '{path}'")]
    NoMatchingRoute { path: String },

    #[error("invalid URI '{uri}'")]
    InvalidUri { uri: String },
}
