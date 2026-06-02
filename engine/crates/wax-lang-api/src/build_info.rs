//! Build metadata shared by the CLI and language packs.

/// Returns the version advertised by built binaries and scan facts.
///
/// Release builds set `WAX_BUILD_VERSION` from the release tag. Local builds use
/// the Cargo package version, which intentionally defaults to a snapshot value.
#[must_use]
pub fn build_version() -> &'static str {
    option_env!("WAX_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}
