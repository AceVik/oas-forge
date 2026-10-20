use oas_forge::index::{Blueprint, Fragment, Registry};

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn new_registry_is_empty() {
    let registry = Registry::new();
    assert!(registry.fragments.is_empty());
    assert!(registry.blueprints.is_empty());
    assert!(registry.schemas.is_empty());
    assert!(registry.concrete_schemas.is_empty());
}

#[test]
fn inserts_are_keyed_by_name_and_replace_existing_entries() {
    let mut registry = Registry::new();
    registry.insert_fragment("F".into(), strings(&["a"]), "old".into());
    registry.insert_fragment("F".into(), strings(&["b"]), "new".into());
    registry.insert_blueprint("Page".into(), strings(&["T"]), "items: $T".into());
    registry.insert_schema("User".into(), "type: object".into());

    assert_eq!(registry.fragments.len(), 1);
    assert_eq!(registry.fragments["F"].params, ["b"]);
    assert_eq!(registry.fragments["F"].body, "new");
    assert_eq!(registry.blueprints["Page"].params, ["T"]);
    assert_eq!(registry.blueprints["Page"].body, "items: $T");
    assert_eq!(registry.schemas["User"], "type: object");
}

#[test]
fn templates_round_trip_through_serde() {
    // Blueprints and fragments are shipped between crates as JSON/YAML.
    let blueprint = Blueprint {
        params: strings(&["T"]),
        body: "items: $T".into(),
    };
    let json = serde_json::to_value(&blueprint).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "params": ["T"], "body": "items: $T" })
    );

    let back: Blueprint = serde_json::from_value(json).unwrap();
    assert_eq!((back.params, back.body), (blueprint.params, blueprint.body));

    let fragment: Fragment =
        serde_yaml_ng::from_str("params: [code]\nbody: 'description: {{code}}'").unwrap();
    assert_eq!(fragment.params, ["code"]);
    assert_eq!(fragment.body, "description: {{code}}");
}
