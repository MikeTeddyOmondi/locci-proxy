use crate::config::UnifiedConfig;

pub mod gateway;
pub mod lb;

pub(crate) fn next_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Lightweight handle used by the control API.
pub struct ServiceManager {
    pub config: UnifiedConfig,
}

impl ServiceManager {
    pub fn new(config: UnifiedConfig) -> Self {
        Self { config }
    }
}
