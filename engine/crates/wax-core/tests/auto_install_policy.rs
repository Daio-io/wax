use std::collections::{BTreeMap, BTreeSet};

use wax_contract::LanguageId;
use wax_core::auto_install::{
    AutoInstallPolicyError, AutoInstallPolicyInput, InstallPlan, evaluate_auto_install_policy,
};
use wax_core::config::lockfile::{LockedLanguage, ResolvedLanguage};

fn language_id(value: &str) -> LanguageId {
    LanguageId::try_from(value).unwrap()
}

fn locked_language(version: &str, sha256: &str) -> LockedLanguage {
    LockedLanguage {
        version: version.to_owned(),
        api_version: 1,
        source: "pack-index".to_owned(),
        resolved: ResolvedLanguage {
            target: "x86_64-unknown-linux-gnu".to_owned(),
            url: "https://example.com/pack.tar.gz".to_owned(),
            sha256: sha256.to_owned(),
            signature: None,
        },
    }
}

#[test]
fn enabled_packs_require_lockfile_entries() {
    let react = language_id("react");

    let decision = evaluate_auto_install_policy(&AutoInstallPolicyInput {
        enabled_language_ids: [react.clone()].into(),
        locked_languages: BTreeMap::new(),
        installed_versions: BTreeMap::new(),
        allow_auto_install: true,
        pack_index_digests: BTreeMap::new(),
    });

    assert_eq!(decision.ready, BTreeSet::new());
    assert_eq!(decision.needs_install, Vec::<InstallPlan>::new());
    assert_eq!(
        decision.errors,
        vec![AutoInstallPolicyError::MissingLockfileEntry { language_id: react }]
    );
}

#[test]
fn no_auto_install_fails_when_enabled_pack_missing_locally() {
    let react = language_id("react");
    let locked_version = "1.2.3";

    let decision = evaluate_auto_install_policy(&AutoInstallPolicyInput {
        enabled_language_ids: [react.clone()].into(),
        locked_languages: BTreeMap::from([(
            react.clone(),
            locked_language(locked_version, "aaaa"),
        )]),
        installed_versions: BTreeMap::new(),
        allow_auto_install: false,
        pack_index_digests: BTreeMap::new(),
    });

    assert_eq!(decision.ready, BTreeSet::new());
    assert_eq!(decision.needs_install, Vec::<InstallPlan>::new());
    assert_eq!(
        decision.errors,
        vec![AutoInstallPolicyError::MissingInstalledWithAutoInstallDisabled {
            language_id: react,
            version: locked_version.to_owned(),
        }]
    );
}

#[test]
fn auto_install_uses_exact_locked_version_and_digest() {
    let react = language_id("react");
    let locked_version = "1.2.3";
    let locked_sha = "abcd1234";

    let decision = evaluate_auto_install_policy(&AutoInstallPolicyInput {
        enabled_language_ids: [react.clone()].into(),
        locked_languages: BTreeMap::from([(
            react.clone(),
            locked_language(locked_version, locked_sha),
        )]),
        installed_versions: BTreeMap::new(),
        allow_auto_install: true,
        pack_index_digests: BTreeMap::from([(
            react.clone(),
            BTreeMap::from([(locked_version.to_owned(), locked_sha.to_owned())]),
        )]),
    });

    assert_eq!(decision.errors, Vec::<AutoInstallPolicyError>::new());
    assert_eq!(decision.ready, BTreeSet::new());
    assert_eq!(
        decision.needs_install,
        vec![InstallPlan {
            language_id: react,
            version: locked_version.to_owned(),
            sha256: locked_sha.to_owned(),
        }]
    );
}

#[test]
fn digest_drift_refuses_install_even_when_auto_install_is_enabled() {
    let react = language_id("react");
    let locked_version = "1.2.3";
    let lockfile_sha = "lock-sha";
    let pack_index_sha = "index-sha";

    let decision = evaluate_auto_install_policy(&AutoInstallPolicyInput {
        enabled_language_ids: [react.clone()].into(),
        locked_languages: BTreeMap::from([(
            react.clone(),
            locked_language(locked_version, lockfile_sha),
        )]),
        installed_versions: BTreeMap::new(),
        allow_auto_install: true,
        pack_index_digests: BTreeMap::from([(
            react.clone(),
            BTreeMap::from([(locked_version.to_owned(), pack_index_sha.to_owned())]),
        )]),
    });

    assert_eq!(decision.ready, BTreeSet::new());
    assert_eq!(decision.needs_install, Vec::<InstallPlan>::new());
    assert_eq!(
        decision.errors,
        vec![AutoInstallPolicyError::DigestDrift {
            language_id: react,
            version: locked_version.to_owned(),
            lockfile_sha256: lockfile_sha.to_owned(),
            pack_index_sha256: pack_index_sha.to_owned(),
        }]
    );
}
