use crate::config::*;

#[test]
fn test_default_config() {
    let cfg = AgmConfig::default();
    assert_eq!(cfg.default_registry, "https://registry.agm.dev");
    assert_eq!(cfg.default_target, "claude");
    assert_eq!(cfg.concurrency, 4);
}

#[test]
fn test_config_roundtrip() {
    let cfg = AgmConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: AgmConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.default_registry, cfg.default_registry);
    assert_eq!(parsed.concurrency, cfg.concurrency);
}
