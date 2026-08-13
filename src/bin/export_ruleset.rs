use narrator_validator::{
    resolve_ruleset, RulesetReference, STANDARD_MYSTERY_RULESET_ID,
    STANDARD_MYSTERY_RULESET_VERSION,
};

fn main() {
    let version = std::env::args()
        .nth(1)
        .unwrap_or_else(|| STANDARD_MYSTERY_RULESET_VERSION.to_string());
    let reference = RulesetReference {
        id: STANDARD_MYSTERY_RULESET_ID.to_string(),
        version,
    };
    let resolved = resolve_ruleset(&reference).expect("standard ruleset resolves");
    let document: serde_yaml::Value =
        serde_yaml::from_str(resolved.commands_yaml).expect("standard ruleset YAML");
    let commands = document["commands"].clone();
    let payload = serde_json::json!({
        "id": reference.id,
        "version": reference.version,
        "commands": commands,
    });
    println!("{}", serde_json::to_string(&payload).expect("ruleset JSON"));
}
