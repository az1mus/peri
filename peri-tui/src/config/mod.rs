// Re-export config types from peri-acp (single source of truth)
// Re-export store functions from peri-acp
pub use peri_acp::provider::{config_path, load, load_from, save, save_to, workspace_config_path};
pub use peri_acp::provider::{
    AppConfig, PeriConfig, ProviderConfig, ProviderModels, ThinkingConfig,
};

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
