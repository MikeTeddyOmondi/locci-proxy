use crate::config::UnifiedConfig;

pub mod gateway;
pub mod lb;

/// Lightweight handle used by the control API.
pub struct ServiceManager {
    pub config: UnifiedConfig,
}

impl ServiceManager {
    pub fn new(config: UnifiedConfig) -> Self {
        Self { config }
    }
}
