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
    let mut payload = serde_json::json!({
        "id": reference.id,
        "version": reference.version,
        "commands": commands,
        "command_capabilities": resolved.command_capabilities,
    });
    if let Some(answers_yaml) = resolved.answers_yaml {
        let answers_document: serde_yaml::Value =
            serde_yaml::from_str(answers_yaml).expect("answer deck YAML");
        payload["answers"] =
            serde_json::to_value(answers_document["answers"].clone()).expect("answer deck JSON");
    }
    println!("{}", serde_json::to_string(&payload).expect("ruleset JSON"));
}
