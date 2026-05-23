use crate::{CliConfigError, HostProfileConfig, parse_profile_config_value};
use jsonschema::Validator;
use schemars::schema_for;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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

pub fn validate_profile_config_schema(value: &Value) -> Result<(), CliConfigError> {
    ProfileConfigValidator::new()?.validate_value(value)
}

pub fn parse_validated_profile_config_text(
    text: &str,
) -> Result<HostProfileConfig, CliConfigError> {
    ProfileConfigValidator::new()?.parse_text(text)
}

#[derive(Clone, Debug)]
pub struct ProfileConfigValidator {
    validator: Validator,
}

impl ProfileConfigValidator {
    pub fn new() -> Result<Self, CliConfigError> {
        let schema = profile_config_schema_value();
        let validator = jsonschema::validator_for(&schema)
            .map_err(|error| CliConfigError::ParseConfig(error.to_string()))?;
        Ok(Self { validator })
    }

    pub fn parse_text(&self, text: &str) -> Result<HostProfileConfig, CliConfigError> {
        let value = parse_profile_config_value(text)?;
        self.validate_value(&value)?;
        let config = serde_json::from_value::<HostProfileConfig>(value)
            .map_err(|error| CliConfigError::ParseConfig(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate_value(&self, value: &Value) -> Result<(), CliConfigError> {
        let errors = self
            .validator
            .iter_errors(value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CliConfigError::ParseConfig(errors.join("; ")))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileConfigSchemaPathSegment {
    Key(String),
    ArrayItem,
}

impl ProfileConfigSchemaPathSegment {
    fn key(value: impl Into<String>) -> Self {
        Self::Key(value.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileConfigSchemaCompletionKind {
    Property,
    Value,
    Snippet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileConfigSchemaCompletion {
    pub label: String,
    pub insert_text: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub kind: ProfileConfigSchemaCompletionKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileConfigSchemaCompletionSet {
    pub replace_start: usize,
    pub completions: Vec<ProfileConfigSchemaCompletion>,
}

#[derive(Clone, Debug)]
pub struct ProfileConfigSchemaIndex {
    schema: Value,
}

impl Default for ProfileConfigSchemaIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileConfigSchemaIndex {
    pub fn new() -> Self {
        Self {
            schema: profile_config_schema_value(),
        }
    }

    pub fn property_completions(
        &self,
        path: &[ProfileConfigSchemaPathSegment],
        used_keys: &BTreeSet<String>,
    ) -> Vec<ProfileConfigSchemaCompletion> {
        let mut completions = BTreeMap::new();
        for schema in self.schemas_for_path(path) {
            for schema in self.expand_schema(&schema) {
                let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
                    continue;
                };
                for (name, property_schema) in properties {
                    if used_keys.contains(name) {
                        continue;
                    }
                    completions.entry(name.clone()).or_insert_with(|| {
                        ProfileConfigSchemaCompletion {
                            label: name.clone(),
                            insert_text: format!("\"{name}\": "),
                            detail: property_detail(property_schema),
                            documentation: property_documentation(property_schema),
                            kind: ProfileConfigSchemaCompletionKind::Property,
                        }
                    });
                }
            }
        }
        completions.into_values().collect()
    }

    pub fn value_completions(
        &self,
        path: &[ProfileConfigSchemaPathSegment],
        key: &str,
    ) -> Vec<ProfileConfigSchemaCompletion> {
        let mut completions = BTreeMap::new();
        for schema in self.property_schemas(path, key) {
            for schema in self.expand_schema(&schema) {
                self.collect_value_completions(&schema, &mut completions);
            }
        }
        completions.into_values().collect()
    }

    pub fn snippet_completions(
        &self,
        path: &[ProfileConfigSchemaPathSegment],
    ) -> Vec<ProfileConfigSchemaCompletion> {
        match path {
            [ProfileConfigSchemaPathSegment::Key(key)] if key == "profiles" => {
                vec![ProfileConfigSchemaCompletion {
                    label: "profile".into(),
                    insert_text: r#"{
  "profileId": "new-profile",
  "displayName": "New Profile",
  "provider": {
    "type": "chatgpt_responses",
    "model": "gpt-5.4-mini"
  },
  "compaction": {
    "type": "auto"
  }
}"#
                    .into(),
                    detail: Some("RuntimeProfileConfig".into()),
                    documentation: Some("Minimal runtime profile object.".into()),
                    kind: ProfileConfigSchemaCompletionKind::Snippet,
                }]
            }
            [
                ProfileConfigSchemaPathSegment::Key(profiles),
                ProfileConfigSchemaPathSegment::ArrayItem,
                ProfileConfigSchemaPathSegment::Key(manifest_patches),
            ] if profiles == "profiles" && manifest_patches == "manifestPatches" => {
                let mut item_path = path.to_vec();
                item_path.push(ProfileConfigSchemaPathSegment::ArrayItem);
                self.value_completions(&item_path, "op")
                    .into_iter()
                    .map(|completion| {
                        let value = completion.label;
                        ProfileConfigSchemaCompletion {
                            label: value.clone(),
                            insert_text: format!(r#"{{ "op": "{value}" }}"#),
                            detail: Some("ManifestPatch".into()),
                            documentation: completion.documentation,
                            kind: ProfileConfigSchemaCompletionKind::Snippet,
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn completions_for_text(
        &self,
        text: &str,
        offset: usize,
    ) -> ProfileConfigSchemaCompletionSet {
        let context = JsoncCompletionScanner::scan(text, offset.min(text.len()));
        let mut completions = match context.container_kind {
            Some(JsonContainerKind::Object) if context.current_key.is_none() => {
                self.property_completions(&context.path, &context.used_keys)
            }
            Some(JsonContainerKind::Object) => context
                .current_key
                .as_deref()
                .map(|key| self.value_completions(&context.path, key))
                .unwrap_or_default(),
            Some(JsonContainerKind::Array) => self.snippet_completions(&context.path),
            None => Vec::new(),
        };
        sort_completions(&mut completions, context.typed_fragment.as_deref());
        ProfileConfigSchemaCompletionSet {
            replace_start: context.replace_start,
            completions,
        }
    }

    fn schemas_for_path(&self, path: &[ProfileConfigSchemaPathSegment]) -> Vec<Value> {
        let mut schemas = vec![self.schema.clone()];
        for segment in path {
            let mut next = Vec::new();
            for schema in schemas {
                for schema in self.expand_schema(&schema) {
                    match segment {
                        ProfileConfigSchemaPathSegment::Key(key) => {
                            next.extend(self.property_schemas_from_schema(&schema, key));
                        }
                        ProfileConfigSchemaPathSegment::ArrayItem => {
                            if let Some(items) = schema.get("items") {
                                next.push(items.clone());
                            }
                        }
                    }
                }
            }
            schemas = next;
            if schemas.is_empty() {
                break;
            }
        }
        schemas
    }

    fn property_schemas(&self, path: &[ProfileConfigSchemaPathSegment], key: &str) -> Vec<Value> {
        self.schemas_for_path(path)
            .into_iter()
            .flat_map(|schema| {
                self.expand_schema(&schema)
                    .into_iter()
                    .flat_map(|schema| self.property_schemas_from_schema(&schema, key))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn property_schemas_from_schema(&self, schema: &Value, key: &str) -> Vec<Value> {
        let mut schemas = Vec::new();
        for schema in self.expand_schema(schema) {
            if let Some(property) = schema
                .get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get(key))
            {
                schemas.push(property.clone());
            }
        }
        schemas
    }

    fn expand_schema(&self, schema: &Value) -> Vec<Value> {
        if schema.get("type").and_then(Value::as_str) == Some("null") {
            return Vec::new();
        }
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str)
            && let Some(resolved) = self.resolve_ref(reference)
        {
            return self.expand_schema(resolved);
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(variants) = schema.get(key).and_then(Value::as_array) {
                return variants
                    .iter()
                    .flat_map(|variant| self.expand_schema(variant))
                    .collect();
            }
        }
        vec![schema.clone()]
    }

    fn resolve_ref(&self, reference: &str) -> Option<&Value> {
        let name = reference.strip_prefix("#/$defs/")?;
        self.schema.get("$defs")?.get(name)
    }

    fn collect_value_completions(
        &self,
        schema: &Value,
        completions: &mut BTreeMap<String, ProfileConfigSchemaCompletion>,
    ) {
        if let Some(value) = schema.get("const") {
            insert_value_completion(completions, value, "const");
        }
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            for value in values {
                insert_value_completion(completions, value, "enum");
            }
        }
        if schema.get("type").and_then(Value::as_str) == Some("boolean") {
            insert_raw_completion(completions, "true", "true", "boolean");
            insert_raw_completion(completions, "false", "false", "boolean");
        }
        if let Some(types) = schema.get("type").and_then(Value::as_array)
            && types.iter().any(|value| value.as_str() == Some("null"))
        {
            insert_raw_completion(completions, "null", "null", "null");
        }
        for variant in self.expand_schema(schema) {
            if let Some(type_const) = variant
                .get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get("type"))
                .and_then(|type_schema| type_schema.get("const"))
            {
                let Some(label) = schema_label(type_const) else {
                    continue;
                };
                completions
                    .entry(label.clone())
                    .or_insert_with(|| ProfileConfigSchemaCompletion {
                        label: label.clone(),
                        insert_text: format!(r#"{{ "type": "{}" }}"#, label),
                        detail: Some("object".into()),
                        documentation: property_documentation(&variant),
                        kind: ProfileConfigSchemaCompletionKind::Snippet,
                    });
            }
        }
    }
}

fn insert_value_completion(
    completions: &mut BTreeMap<String, ProfileConfigSchemaCompletion>,
    value: &Value,
    detail: &'static str,
) {
    let Some(label) = schema_label(value) else {
        return;
    };
    let insert_text = match value {
        Value::String(value) => format!("\"{value}\""),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "null".into(),
        _ => return,
    };
    insert_raw_completion(completions, &label, insert_text, detail);
}

fn insert_raw_completion(
    completions: &mut BTreeMap<String, ProfileConfigSchemaCompletion>,
    label: &str,
    insert_text: impl Into<String>,
    detail: &'static str,
) {
    completions
        .entry(label.to_string())
        .or_insert_with(|| ProfileConfigSchemaCompletion {
            label: label.to_string(),
            insert_text: insert_text.into(),
            detail: Some(detail.into()),
            documentation: None,
            kind: ProfileConfigSchemaCompletionKind::Value,
        });
}

fn schema_label(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Null => Some("null".into()),
        _ => None,
    }
}

fn property_detail(schema: &Value) -> Option<String> {
    schema
        .get("type")
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Array(values) => Some(
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" | "),
            ),
            _ => None,
        })
        .or_else(|| {
            schema
                .get("$ref")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            schema
                .get("anyOf")
                .and_then(Value::as_array)
                .map(|_| "anyOf".to_string())
        })
        .or_else(|| {
            schema
                .get("oneOf")
                .and_then(Value::as_array)
                .map(|_| "oneOf".to_string())
        })
}

fn property_documentation(schema: &Value) -> Option<String> {
    schema
        .get("description")
        .or_else(|| schema.get("title"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonContainerKind {
    Object,
    Array,
}

#[derive(Clone, Debug)]
struct JsonContainer {
    kind: JsonContainerKind,
    path: Vec<ProfileConfigSchemaPathSegment>,
    current_key: Option<String>,
    used_keys: BTreeSet<String>,
    last_string: Option<JsonString>,
}

#[derive(Clone, Debug)]
struct JsonString {
    value: String,
    start: usize,
}

#[derive(Clone, Debug)]
struct JsoncCompletionContext {
    container_kind: Option<JsonContainerKind>,
    path: Vec<ProfileConfigSchemaPathSegment>,
    current_key: Option<String>,
    used_keys: BTreeSet<String>,
    replace_start: usize,
    typed_fragment: Option<String>,
}

struct JsoncCompletionScanner;

impl JsoncCompletionScanner {
    fn scan(text: &str, offset: usize) -> JsoncCompletionContext {
        let mut stack = Vec::<JsonContainer>::new();
        let mut in_string = false;
        let mut escaped = false;
        let mut string_start = 0;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut previous = '\0';
        let mut iter = text[..offset].char_indices().peekable();

        while let Some((index, ch)) = iter.next() {
            if in_line_comment {
                if ch == '\n' {
                    in_line_comment = false;
                }
                previous = ch;
                continue;
            }
            if in_block_comment {
                if previous == '*' && ch == '/' {
                    in_block_comment = false;
                    previous = '\0';
                } else {
                    previous = ch;
                }
                continue;
            }
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                    let end = index + ch.len_utf8();
                    if let Some(container) = stack.last_mut()
                        && container.kind == JsonContainerKind::Object
                        && container.current_key.is_none()
                        && let Ok(value) = serde_json::from_str::<String>(&text[string_start..end])
                    {
                        container.last_string = Some(JsonString {
                            value,
                            start: string_start,
                        });
                    }
                }
                previous = ch;
                continue;
            }
            if ch == '/' {
                match iter.peek().copied() {
                    Some((_, '/')) => {
                        in_line_comment = true;
                        iter.next();
                        previous = '\0';
                        continue;
                    }
                    Some((_, '*')) => {
                        in_block_comment = true;
                        iter.next();
                        previous = '\0';
                        continue;
                    }
                    _ => {}
                }
            }
            match ch {
                '"' => {
                    in_string = true;
                    escaped = false;
                    string_start = index;
                }
                '{' => {
                    let path = path_for_new_container(stack.last(), JsonContainerKind::Object);
                    stack.push(JsonContainer {
                        kind: JsonContainerKind::Object,
                        path,
                        current_key: None,
                        used_keys: BTreeSet::new(),
                        last_string: None,
                    });
                }
                '[' => {
                    let path = path_for_new_container(stack.last(), JsonContainerKind::Array);
                    stack.push(JsonContainer {
                        kind: JsonContainerKind::Array,
                        path,
                        current_key: None,
                        used_keys: BTreeSet::new(),
                        last_string: None,
                    });
                }
                '}' | ']' => {
                    stack.pop();
                }
                ':' => {
                    if let Some(container) = stack.last_mut()
                        && container.kind == JsonContainerKind::Object
                        && let Some(key) = container.last_string.take()
                    {
                        container.used_keys.insert(key.value.clone());
                        container.current_key = Some(key.value);
                    }
                }
                ',' => {
                    if let Some(container) = stack.last_mut()
                        && container.kind == JsonContainerKind::Object
                    {
                        container.current_key = None;
                        container.last_string = None;
                    }
                }
                _ => {}
            }
            previous = ch;
        }

        let Some(container) = stack.last() else {
            return JsoncCompletionContext {
                container_kind: None,
                path: Vec::new(),
                current_key: None,
                used_keys: BTreeSet::new(),
                replace_start: offset,
                typed_fragment: None,
            };
        };
        let (replace_start, typed_fragment) =
            completion_edit_context(text, offset, in_string, string_start, container);
        JsoncCompletionContext {
            container_kind: Some(container.kind),
            path: container.path.clone(),
            current_key: container.current_key.clone(),
            used_keys: container.used_keys.clone(),
            replace_start,
            typed_fragment,
        }
    }
}

fn completion_edit_context(
    text: &str,
    offset: usize,
    in_string: bool,
    string_start: usize,
    container: &JsonContainer,
) -> (usize, Option<String>) {
    if in_string {
        return (
            string_start,
            Some(text[string_start + 1..offset].to_string()),
        );
    }
    if container.kind == JsonContainerKind::Object
        && container.current_key.is_none()
        && let Some(last_string) = container.last_string.as_ref()
    {
        return (last_string.start, Some(last_string.value.clone()));
    }
    (offset, None)
}

fn sort_completions(completions: &mut [ProfileConfigSchemaCompletion], typed: Option<&str>) {
    let typed = normalize_completion_fragment(typed.unwrap_or_default());
    completions.sort_by(|left, right| {
        completion_rank(&typed, &left.label)
            .cmp(&completion_rank(&typed, &right.label))
            .then_with(|| left.label.cmp(&right.label))
    });
}

fn normalize_completion_fragment(typed: &str) -> String {
    typed
        .trim()
        .trim_matches('"')
        .trim_end_matches(':')
        .trim()
        .to_ascii_lowercase()
}

fn completion_rank(typed: &str, label: &str) -> usize {
    if typed.is_empty() {
        return 0;
    }
    let label = label.to_ascii_lowercase();
    if label == typed {
        return 0;
    }
    if label.starts_with(typed) {
        return 1;
    }
    if acronym(&label).starts_with(typed) {
        return 2;
    }
    if label.contains(typed) {
        return 3;
    }
    4 + levenshtein_distance(typed, &label).min(8)
}

fn acronym(label: &str) -> String {
    let mut result = String::new();
    let mut previous_lowercase = false;
    for ch in label.chars() {
        if ch == '_' || ch == '-' {
            previous_lowercase = false;
            continue;
        }
        if result.is_empty() || (previous_lowercase && ch.is_ascii_uppercase()) {
            result.push(ch.to_ascii_lowercase());
        }
        previous_lowercase = ch.is_ascii_lowercase();
    }
    result
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let mut previous = (0..=right.chars().count()).collect::<Vec<_>>();
    let mut current = vec![0; previous.len()];
    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.chars().enumerate() {
            current[right_index + 1] = if left_char == right_char {
                previous[right_index]
            } else {
                1 + previous[right_index]
                    .min(previous[right_index + 1])
                    .min(current[right_index])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.chars().count()]
}

fn path_for_new_container(
    parent: Option<&JsonContainer>,
    kind: JsonContainerKind,
) -> Vec<ProfileConfigSchemaPathSegment> {
    let Some(parent) = parent else {
        return Vec::new();
    };
    let mut path = parent.path.clone();
    match parent.kind {
        JsonContainerKind::Object => {
            if let Some(key) = parent.current_key.as_ref() {
                path.push(ProfileConfigSchemaPathSegment::key(key));
                if kind == JsonContainerKind::Object {
                    return path;
                }
            }
        }
        JsonContainerKind::Array => {
            if kind == JsonContainerKind::Object {
                path.push(ProfileConfigSchemaPathSegment::ArrayItem);
            }
        }
    }
    path
}

#[cfg(test)]
mod tests;
