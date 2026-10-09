#![allow(
    clippy::expect_used,
    reason = "Shared configuration fixtures use expect for precise test diagnostics."
)]

use intention_config::{ConfigPathDto, ConfigSourceDto};

/// Credential-shaped fixture text that must never surface in public data.
pub const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

/// Returns one absolute fixture path under the temporary directory.
pub fn fixture_path(filename: &str) -> String {
    std::env::temp_dir()
        .join(filename)
        .to_string_lossy()
        .into_owned()
}

/// Returns the explicit configuration source every fixture resolves against.
pub fn explicit_source() -> ConfigSourceDto {
    ConfigSourceDto::Explicit(
        ConfigPathDto::parse(fixture_path("intention.toml"))
            .expect("fixture config path is absolute"),
    )
}
