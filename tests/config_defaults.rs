use grpctestify::config::Config;

#[test]
fn test_default_config_values() {
    let config = Config::default();

    // Verify defaults match legacy bash version
    assert_eq!(config.general.address, "localhost:4770");
    assert_eq!(config.general.parallel, "auto");
    assert_eq!(config.general.timeout, 30);
    assert_eq!(config.general.retry, 3);
    assert_eq!(config.general.retry_delay, 1.0);
}
