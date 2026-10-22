use wax_core::{RegistryLookupError, RegistryLookupErrorCode};

#[test]
fn shared_lookup_error_is_exported_from_core() {
    let error = RegistryLookupError::new(
        RegistryLookupErrorCode::OfflineCacheMiss,
        "snapshot is not cached",
    );
    assert_eq!(error.code, RegistryLookupErrorCode::OfflineCacheMiss);
    assert_eq!(error.message, "snapshot is not cached");
    assert!(error.diagnostics.is_empty());
    let encoded = serde_json::to_value(&error).expect("serialize");
    assert_eq!(encoded["code"], "offline_cache_miss");
}
