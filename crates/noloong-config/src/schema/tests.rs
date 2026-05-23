use super::{
    ProfileConfigSchemaCompletionKind, ProfileConfigSchemaIndex, ProfileConfigSchemaPathSegment,
    parse_validated_profile_config_text, profile_config_schema_json, profile_config_schema_value,
    validate_profile_config_schema,
};
use crate::{parse_profile_config_text, parse_profile_config_value};
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
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/profile-configs");

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

#[test]
fn profile_config_text_parser_is_public_and_accepts_jsonc() {
    let config = parse_profile_config_text(
        r#"{
                // JSONC comment
                "profiles": [
                    {
                        "profileId": "default",
                        "displayName": "Default",
                        "provider": {
                            "type": "chatgpt_responses",
                            "model": "gpt-5.4-mini",
                        },
                    },
                ],
            }"#,
    )
    .unwrap();

    assert_eq!(config.profiles[0].profile_id, "default");
}

#[test]
fn profile_config_schema_validation_reports_invalid_enum() {
    let value = serde_json::json!({
        "profiles": [
            {
                "profileId": "default",
                "displayName": "Default",
                "provider": {
                    "type": "bogus",
                    "model": "gpt-5.4-mini"
                }
            }
        ]
    });

    let error = validate_profile_config_schema(&value).unwrap_err();

    assert!(error.to_string().contains("bogus"));
}

#[test]
fn validated_profile_config_text_checks_schema_and_typed_rules() {
    let config = parse_validated_profile_config_text(
        r#"{
                "defaultProfileId": "default",
                "profiles": [
                    {
                        "profileId": "default",
                        "displayName": "Default",
                        "provider": {
                            "type": "chatgpt_responses",
                            "model": "gpt-5.4-mini"
                        }
                    }
                ]
            }"#,
    )
    .unwrap();

    assert_eq!(config.default_profile_id.as_deref(), Some("default"));
}

#[test]
fn schema_index_completes_root_and_profile_properties() {
    let index = ProfileConfigSchemaIndex::new();
    let root = index.property_completions(&[], &Default::default());
    assert!(root.iter().any(|completion| completion.label == "profiles"));

    let profile_path = [
        ProfileConfigSchemaPathSegment::Key("profiles".into()),
        ProfileConfigSchemaPathSegment::ArrayItem,
    ];
    let profile = index.property_completions(&profile_path, &Default::default());
    assert!(
        profile
            .iter()
            .any(|completion| completion.label == "profileId")
    );
    assert!(
        profile
            .iter()
            .any(|completion| completion.label == "provider")
    );
}

#[test]
fn schema_index_completes_provider_and_compaction_type_values() {
    let index = ProfileConfigSchemaIndex::new();
    let provider_path = [
        ProfileConfigSchemaPathSegment::Key("profiles".into()),
        ProfileConfigSchemaPathSegment::ArrayItem,
        ProfileConfigSchemaPathSegment::Key("provider".into()),
    ];
    let provider_types = index.value_completions(&provider_path, "type");
    assert!(
        provider_types
            .iter()
            .any(|completion| completion.label == "chatgpt_responses"
                && completion.insert_text == "\"chatgpt_responses\"")
    );

    let compaction_path = [
        ProfileConfigSchemaPathSegment::Key("profiles".into()),
        ProfileConfigSchemaPathSegment::ArrayItem,
        ProfileConfigSchemaPathSegment::Key("compaction".into()),
    ];
    let compaction_types = index.value_completions(&compaction_path, "type");
    assert!(
        compaction_types
            .iter()
            .any(|completion| completion.label == "auto")
    );
}

#[test]
fn schema_index_completes_profile_and_manifest_patch_snippets() {
    let index = ProfileConfigSchemaIndex::new();
    let profile_snippets =
        index.snippet_completions(&[ProfileConfigSchemaPathSegment::Key("profiles".into())]);
    assert_eq!(
        profile_snippets[0].kind,
        ProfileConfigSchemaCompletionKind::Snippet
    );
    assert!(profile_snippets[0].insert_text.contains("\"provider\""));

    let manifest_snippets = index.snippet_completions(&[
        ProfileConfigSchemaPathSegment::Key("profiles".into()),
        ProfileConfigSchemaPathSegment::ArrayItem,
        ProfileConfigSchemaPathSegment::Key("manifestPatches".into()),
    ]);
    assert!(
        manifest_snippets
            .iter()
            .any(|completion| completion.label == "set_locale")
    );
}

#[test]
fn schema_index_scans_text_context_for_completions() {
    let index = ProfileConfigSchemaIndex::new();
    let text = r#"{
  "profiles": [
    {
      "provider": {
        "type": 
      }
    }
  ]
}"#;
    let offset = text.find("\"type\": ").unwrap() + "\"type\": ".len();
    let completions = index.completions_for_text(text, offset);

    assert!(
        completions
            .completions
            .iter()
            .any(|completion| completion.label == "chatgpt_responses")
    );
}

#[test]
fn schema_index_replaces_in_progress_property_key() {
    let index = ProfileConfigSchemaIndex::new();
    let text = r#"{
  "profiles": [
    {"prifileId"}
  ]
}"#;
    let offset = text.find("\"prifileId\"").unwrap() + "\"prifileId\"".len();
    let completions = index.completions_for_text(text, offset);

    assert_eq!(
        completions.replace_start,
        text.find("\"prifileId\"").unwrap()
    );
    assert_eq!(completions.completions[0].label, "profileId");
    assert_eq!(completions.completions[0].insert_text, "\"profileId\": ");
}

#[test]
fn schema_index_prioritizes_typed_property_prefix_inside_quotes() {
    let index = ProfileConfigSchemaIndex::new();
    let text = r#"{
  "profiles": [
    {"pro"}
  ]
}"#;
    let offset = text.find("\"pro\"").unwrap() + "\"pro".len();
    let completions = index.completions_for_text(text, offset);

    assert_eq!(completions.completions[0].label, "profileId");
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
