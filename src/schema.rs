use crate::config::HostProfileConfig;
use schemars::schema_for;
use serde_json::Value;

const PROFILE_CONFIG_SCHEMA_TITLE: &str = "Noloong Profile Config";

pub fn profile_config_schema_value() -> Value {
    let mut value = serde_json::to_value(schema_for!(HostProfileConfig))
        .expect("profile config schema is serializable");
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "title".into(),
            Value::String(PROFILE_CONFIG_SCHEMA_TITLE.into()),
        );
    }
    value
}

pub fn profile_config_schema_json() -> String {
    let mut text = serde_json::to_string_pretty(&profile_config_schema_value())
        .expect("profile config schema JSON is serializable");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::{profile_config_schema_json, profile_config_schema_value};
    use crate::config::parse_profile_config_value;
    use serde_json::Value;
    use std::{fs, path::Path};

    #[test]
    fn profile_config_schema_contains_root_contract() {
        let schema = profile_config_schema_value();

        assert_eq!(schema["title"].as_str(), Some("Noloong Profile Config"));
        assert!(schema.get("$schema").is_some());
        assert!(schema.get("$defs").is_some());
        assert!(schema["properties"].get("profiles").is_some());
    }

    #[test]
    fn profile_config_schema_json_is_parseable() {
        let text = profile_config_schema_json();

        let parsed: Value = serde_json::from_str(&text).unwrap();

        assert_eq!(parsed["title"].as_str(), Some("Noloong Profile Config"));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn profile_config_examples_validate_against_schema() {
        let schema = profile_config_schema_value();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/profile-configs");

        for entry in fs::read_dir(examples_dir).unwrap() {
            let path = entry.unwrap().path();
            let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
                continue;
            };
            if !matches!(extension, "json" | "jsonc") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            let value = parse_profile_example_value(extension, &text)
                .unwrap_or_else(|error| panic!("{} failed to parse: {error}", path.display()));
            let errors = validator.iter_errors(&value).collect::<Vec<_>>();
            assert!(
                errors.is_empty(),
                "{} failed schema validation: {errors:?}",
                path.display()
            );
        }
    }

    fn parse_profile_example_value(
        extension: &str,
        text: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        match extension {
            "json" => Ok(serde_json::from_str(text)?),
            "jsonc" => Ok(parse_profile_config_value(text)?),
            _ => unreachable!("example extension is filtered before parsing"),
        }
    }
}
