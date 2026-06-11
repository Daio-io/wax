use wax_contract::LanguageId;
use wax_core::config::repo_files::default_registry_path_for_language;

#[test]
fn default_registry_path_uses_language_id_slug() {
    let compose = LanguageId::try_from("compose").unwrap();
    let react = LanguageId::try_from("react").unwrap();

    assert_eq!(
        default_registry_path_for_language(&compose),
        ".wax/compose.registry.json"
    );
    assert_eq!(
        default_registry_path_for_language(&react),
        ".wax/react.registry.json"
    );
}
