//! Shared default values used across `wax` crates.

/// Default language pack index for alpha releases.
///
/// This points at the branch-backed raw URL for this repository's published
/// `index.json`: `https://raw.githubusercontent.com/Daio-io/wax/gh-pages/index.json`.
/// The release workflow bootstraps and updates that branch when publishing
/// alpha tags. Until the next React-inclusive alpha tag ships, the hosted file
/// may list only `compose` and `basic`; `react` appears after that publish.
pub const DEFAULT_WAX_PACK_INDEX: &str =
    "https://raw.githubusercontent.com/Daio-io/wax/gh-pages/index.json";
