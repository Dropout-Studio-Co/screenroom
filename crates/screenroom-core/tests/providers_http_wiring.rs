use screenroom_core::{
    ApiCredential, CatalogueMetadataProvider, ProviderConfig, Secret, SubtitleProvider,
    SubtitleProviderConfig, TmdbProvider, TsdbProvider,
};

fn credential() -> ApiCredential {
    ApiCredential::ApiKey(Secret::new("unit-test-key"))
}

#[test]
fn the_default_metadata_transport_carries_the_default_secure_address() {
    let config = ProviderConfig::configured(credential());
    assert!(
        config.base_url.starts_with("https://"),
        "{}",
        config.base_url
    );

    let capability = TmdbProvider::new(config).capability();

    assert!(capability.is_configured(), "{}", capability.detail);
    assert_eq!(capability.provider, "tmdb");
}

#[test]
fn the_default_subtitle_transport_carries_the_default_secure_address() {
    let config = SubtitleProviderConfig::unconfigured().with_api_key("unit-test-key");
    assert!(
        config.base_url.starts_with("https://"),
        "{}",
        config.base_url
    );

    let capability = TsdbProvider::new(config).capability();

    assert!(capability.is_configured(), "{}", capability.detail);
    assert_eq!(capability.provider, "thesubtitledb");
}

#[test]
fn a_scheme_the_transport_cannot_carry_still_fails_closed() {
    let config = ProviderConfig::configured(credential()).with_base_url("ftp://api.invalid/3");

    let capability = TmdbProvider::new(config).capability();

    assert!(!capability.is_configured());
    assert!(capability.detail.contains("ftp"), "{}", capability.detail);
}

#[test]
fn a_plain_address_is_still_carried_so_a_local_endpoint_keeps_working() {
    let config = ProviderConfig::configured(credential()).with_base_url("http://api.invalid/3");

    let capability = TmdbProvider::new(config).capability();

    assert!(capability.is_configured(), "{}", capability.detail);
}
