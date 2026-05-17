use crate::{plugin::SkillsPluginComponent, system_prompt};
use noloong_agent_core::{AgentMessage, ContentBlock, MessageRole};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

const SKILL_FILE_NAME: &str = "SKILL.md";
const DEFAULT_MAX_SCAN_DEPTH: usize = 6;
const DEFAULT_MAX_SCAN_DIRS: usize = 1024;
const MAX_NAME_CHARS: usize = 128;
const MAX_DESCRIPTION_CHARS: usize = 1200;
const TOKEN_CHAR_RATIO: usize = 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSkills {
    pub skills: Vec<SkillMetadata>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl LoadedSkills {
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn merge(&mut self, other: LoadedSkills) {
        self.skills.extend(other.skills);
        self.warnings.extend(other.warnings);
        self.sort_and_dedup();
    }

    pub fn sort_and_dedup(&mut self) {
        self.skills.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.plugin_id.cmp(&right.plugin_id))
        });
        let mut seen_paths = BTreeSet::new();
        self.skills
            .retain(|skill| seen_paths.insert(skill.path.clone()));
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub root: PathBuf,
    pub plugin_id: String,
    pub scope: String,
    #[serde(skip)]
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillRender {
    pub text: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum SkillLoadError {
    #[error("skills root `{path}` is not a directory")]
    RootNotDirectory { path: String },
    #[error("failed to read skills root `{path}`: {source}")]
    ReadRoot {
        path: String,
        source: std::io::Error,
    },
    #[error("skill `{path}` is invalid: {message}")]
    InvalidSkill { path: String, message: String },
}

pub fn load_plugin_skills(
    plugin_id: &str,
    component: &SkillsPluginComponent,
    cwd: &Path,
) -> Result<LoadedSkills, SkillLoadError> {
    let mut loaded = LoadedSkills::default();
    for root in &component.roots {
        let root = absolute_path(cwd, root);
        let root = canonical_or_original(root);
        let mut scanner = SkillScanner::new(plugin_id, root);
        loaded.merge(scanner.scan()?);
    }
    Ok(loaded)
}

pub fn render_skills_instructions(
    loaded: &LoadedSkills,
    input_limit_tokens: u64,
) -> Option<SkillRender> {
    if loaded.skills.is_empty() {
        return None;
    }
    let budget_tokens = ((input_limit_tokens as f64) * 0.02).floor().max(1.0) as usize;
    let budget_chars = budget_tokens.saturating_mul(TOKEN_CHAR_RATIO).max(1);
    Some(render_with_budget(loaded, budget_chars))
}

pub fn render_explicit_skill_instructions(
    loaded: &LoadedSkills,
    messages: &[AgentMessage],
) -> Option<SkillRender> {
    if loaded.skills.is_empty() {
        return None;
    }
    let message = latest_user_message(messages)?;
    let matches = explicit_skill_matches(loaded, message);
    if matches.is_empty() {
        return None;
    }

    let warnings = Vec::new();
    let mut blocks = Vec::new();
    for skill in matches {
        blocks.push(format!(
            "<skill name=\"{}\" path=\"{}\">\n{}\n</skill>",
            system_prompt::escape_xml_attribute(&skill.name),
            system_prompt::escape_xml_attribute(&skill.path.display().to_string()),
            skill.body.trim()
        ));
    }
    if blocks.is_empty() && warnings.is_empty() {
        return None;
    }
    let text = if blocks.is_empty() {
        String::new()
    } else {
        format!(
            "<skill_instructions>\n{}\n</skill_instructions>",
            blocks.join("\n")
        )
    };
    Some(SkillRender { text, warnings })
}

fn render_with_budget(loaded: &LoadedSkills, budget_chars: usize) -> SkillRender {
    let full = render_metadata(loaded, PathRenderMode::Absolute, None);
    if full.len() <= budget_chars {
        return SkillRender {
            text: wrap_available_skills(&full),
            warnings: loaded.warnings.clone(),
        };
    }

    let aliased = render_metadata(loaded, PathRenderMode::Aliased, None);
    if aliased.len() <= budget_chars {
        let mut warnings = loaded.warnings.clone();
        warnings.push("skills metadata used root aliases to fit the 2% input-window budget".into());
        return SkillRender {
            text: wrap_available_skills(&aliased),
            warnings,
        };
    }

    let available_description_chars = budget_chars.saturating_sub(metadata_fixed_chars(loaded));
    let per_description = (available_description_chars / loaded.skills.len()).max(24);
    let truncated = render_metadata(loaded, PathRenderMode::Aliased, Some(per_description));
    if truncated.len() <= budget_chars {
        let mut warnings = loaded.warnings.clone();
        warnings.push(
            "skills metadata descriptions were truncated to fit the 2% input-window budget".into(),
        );
        return SkillRender {
            text: wrap_available_skills(&truncated),
            warnings,
        };
    }

    let minimum = render_minimum_rows(loaded, budget_chars);
    let mut warnings = loaded.warnings.clone();
    warnings.push("skills metadata exceeded the 2% input-window budget; only minimum rows that fit were rendered".into());
    SkillRender {
        text: wrap_available_skills(&minimum),
        warnings,
    }
}

fn wrap_available_skills(metadata: &str) -> String {
    format!(
        "<skills_instructions>\n\
Available skills are listed below. Each entry includes a readable path. Use a skill when the user explicitly mentions it or when its metadata clearly matches the task.\n\
If you need the full instructions, read the listed SKILL.md path with normal host file/command capabilities. There is no agent.skill.* tool.\n\
When a user explicitly mentions $skill, skill://path, or a structured skill selection, the host may inject that SKILL.md body for the current turn only.\n\n\
{}\n\
</skills_instructions>",
        metadata.trim()
    )
}

fn render_metadata(
    loaded: &LoadedSkills,
    path_mode: PathRenderMode,
    max_description_chars: Option<usize>,
) -> String {
    let aliases = root_aliases(loaded);
    let mut lines = Vec::new();
    if matches!(path_mode, PathRenderMode::Aliased) {
        lines.push("Skill roots:".to_string());
        for (alias, root) in &aliases {
            lines.push(format!("- {alias}: {}", root.display()));
        }
        lines.push("Skills:".to_string());
    } else {
        lines.push("Skills:".to_string());
    }
    for skill in &loaded.skills {
        let description = max_description_chars
            .map(|limit| truncate_chars(&skill.description, limit))
            .unwrap_or_else(|| skill.description.clone());
        lines.push(format!(
            "- name: {}\n  description: {}\n  path: {}\n  pluginId: {}",
            skill.name,
            description,
            rendered_path(skill, path_mode, &aliases),
            skill.plugin_id,
        ));
    }
    lines.join("\n")
}

fn render_minimum_rows(loaded: &LoadedSkills, budget_chars: usize) -> String {
    let aliases = root_aliases(loaded);
    let mut output = String::new();
    output.push_str("Skill roots:\n");
    for (alias, root) in &aliases {
        output.push_str(&format!("- {alias}: {}\n", root.display()));
    }
    output.push_str("Skills:\n");
    for skill in &loaded.skills {
        let row = format!(
            "- {}: {}\n",
            skill.name,
            rendered_path(skill, PathRenderMode::Aliased, &aliases)
        );
        if output.len().saturating_add(row.len()) > budget_chars {
            break;
        }
        output.push_str(&row);
    }
    output
}

fn metadata_fixed_chars(loaded: &LoadedSkills) -> usize {
    loaded
        .skills
        .iter()
        .map(|skill| skill.name.len() + skill.path.display().to_string().len() + 64)
        .sum()
}

fn root_aliases(loaded: &LoadedSkills) -> BTreeMap<String, PathBuf> {
    let mut roots = loaded
        .skills
        .iter()
        .map(|skill| skill.root.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    roots.sort();
    roots
        .into_iter()
        .enumerate()
        .map(|(index, root)| (format!("skill-root-{}", index + 1), root))
        .collect()
}

fn rendered_path(
    skill: &SkillMetadata,
    mode: PathRenderMode,
    aliases: &BTreeMap<String, PathBuf>,
) -> String {
    if matches!(mode, PathRenderMode::Absolute) {
        return skill.path.display().to_string();
    }
    for (alias, root) in aliases {
        if let Ok(relative) = skill.path.strip_prefix(root) {
            return format!("{alias}/{}", relative.display());
        }
    }
    skill.path.display().to_string()
}

fn explicit_skill_matches<'a>(
    loaded: &'a LoadedSkills,
    message: &AgentMessage,
) -> Vec<&'a SkillMetadata> {
    let mut matches = Vec::new();
    let unique_names = unique_names(&loaded.skills);
    for content in &message.content {
        match content {
            ContentBlock::Text { text } => {
                collect_text_skill_matches(loaded, &unique_names, text, &mut matches);
            }
            ContentBlock::Json { value } => {
                collect_structured_skill_matches(loaded, &unique_names, value, &mut matches);
            }
            _ => {}
        }
    }
    matches.sort_by(|left, right| left.path.cmp(&right.path));
    matches.dedup_by(|left, right| left.path == right.path);
    matches
}

fn collect_text_skill_matches<'a>(
    loaded: &'a LoadedSkills,
    unique_names: &BTreeSet<String>,
    text: &str,
    matches: &mut Vec<&'a SkillMetadata>,
) {
    for skill in &loaded.skills {
        if text.contains(&format!("skill://{}", skill.path.display())) {
            matches.push(skill);
            continue;
        }
        if unique_names.contains(&skill.name) && text.contains(&format!("${}", skill.name)) {
            matches.push(skill);
        }
    }
}

fn collect_structured_skill_matches<'a>(
    loaded: &'a LoadedSkills,
    unique_names: &BTreeSet<String>,
    value: &Value,
    matches: &mut Vec<&'a SkillMetadata>,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    if let Some(skills) = object.get("skills").and_then(Value::as_array) {
        for skill in skills {
            collect_structured_skill_matches(loaded, unique_names, skill, matches);
        }
    }
    let selection_type = object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "skill_selection" | "skill" | "selected_skill"));
    if !selection_type {
        return;
    }
    if let Some(path) = object.get("path").and_then(Value::as_str) {
        collect_path_skill_match(loaded, path, matches);
    }
    if let Some(name) = object
        .get("name")
        .or_else(|| object.get("skill"))
        .and_then(Value::as_str)
    {
        collect_name_skill_match(loaded, unique_names, name, matches);
    }
}

fn collect_path_skill_match<'a>(
    loaded: &'a LoadedSkills,
    path: &str,
    matches: &mut Vec<&'a SkillMetadata>,
) {
    let path = PathBuf::from(path);
    for skill in &loaded.skills {
        if skill.path == path {
            matches.push(skill);
        }
    }
}

fn collect_name_skill_match<'a>(
    loaded: &'a LoadedSkills,
    unique_names: &BTreeSet<String>,
    name: &str,
    matches: &mut Vec<&'a SkillMetadata>,
) {
    if !unique_names.contains(name) {
        return;
    }
    for skill in &loaded.skills {
        if skill.name == name {
            matches.push(skill);
        }
    }
}

fn unique_names(skills: &[SkillMetadata]) -> BTreeSet<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for skill in skills {
        *counts.entry(skill.name.clone()).or_default() += 1;
    }
    counts
        .into_iter()
        .filter_map(|(name, count)| (count == 1).then_some(name))
        .collect()
}

fn latest_user_message(messages: &[AgentMessage]) -> Option<&AgentMessage> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::User)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let kept = max_chars.saturating_sub(1);
    format!("{}...", value.chars().take(kept).collect::<String>())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathRenderMode {
    Absolute,
    Aliased,
}

struct SkillScanner {
    plugin_id: String,
    root: PathBuf,
    visited_dirs: usize,
    seen_paths: BTreeSet<PathBuf>,
}

impl SkillScanner {
    fn new(plugin_id: &str, root: PathBuf) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            root,
            visited_dirs: 0,
            seen_paths: BTreeSet::new(),
        }
    }

    fn scan(&mut self) -> Result<LoadedSkills, SkillLoadError> {
        if !self.root.is_dir() {
            return Err(SkillLoadError::RootNotDirectory {
                path: self.root.display().to_string(),
            });
        }
        let mut loaded = LoadedSkills::default();
        let root = self.root.clone();
        self.scan_dir(&root, 0, &mut loaded)?;
        loaded.sort_and_dedup();
        Ok(loaded)
    }

    fn scan_dir(
        &mut self,
        dir: &Path,
        depth: usize,
        loaded: &mut LoadedSkills,
    ) -> Result<(), SkillLoadError> {
        if depth > DEFAULT_MAX_SCAN_DEPTH || is_hidden_path(dir) {
            return Ok(());
        }
        self.visited_dirs += 1;
        if self.visited_dirs > DEFAULT_MAX_SCAN_DIRS {
            loaded.warnings.push(format!(
                "skills scan stopped at {} directories under `{}`",
                DEFAULT_MAX_SCAN_DIRS,
                self.root.display()
            ));
            return Ok(());
        }

        let skill_path = dir.join(SKILL_FILE_NAME);
        if skill_path.is_file() {
            let skill = parse_skill_file(&self.plugin_id, &self.root, &skill_path)?;
            if self.seen_paths.insert(skill.path.clone()) {
                loaded.skills.push(skill);
            }
        }

        let mut entries = fs::read_dir(dir)
            .map_err(|source| SkillLoadError::ReadRoot {
                path: dir.display().to_string(),
                source,
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| SkillLoadError::ReadRoot {
                path: dir.display().to_string(),
                source,
            })?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                self.scan_dir(&path, depth + 1, loaded)?;
            }
        }
        Ok(())
    }
}

fn parse_skill_file(
    plugin_id: &str,
    root: &Path,
    path: &Path,
) -> Result<SkillMetadata, SkillLoadError> {
    let body = fs::read_to_string(path).map_err(|source| SkillLoadError::ReadRoot {
        path: path.display().to_string(),
        source,
    })?;
    let frontmatter = parse_frontmatter(&body).ok_or_else(|| SkillLoadError::InvalidSkill {
        path: path.display().to_string(),
        message: "missing YAML frontmatter with name and description".into(),
    })?;
    let name = required_frontmatter_field(path, &frontmatter, "name")?;
    let description = required_frontmatter_field(path, &frontmatter, "description")?;
    validate_len(path, "name", &name, MAX_NAME_CHARS)?;
    validate_len(path, "description", &description, MAX_DESCRIPTION_CHARS)?;
    Ok(SkillMetadata {
        name,
        description,
        path: canonical_or_original(path.to_path_buf()),
        root: canonical_or_original(root.to_path_buf()),
        plugin_id: plugin_id.into(),
        scope: "plugin".into(),
        body,
    })
}

fn parse_frontmatter(body: &str) -> Option<BTreeMap<String, String>> {
    let rest = body.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];
    let mut fields = BTreeMap::new();
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        fields.insert(key.to_owned(), strip_yaml_scalar(value.trim()));
    }
    Some(fields)
}

fn strip_yaml_scalar(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
        .trim()
        .to_owned()
}

fn required_frontmatter_field(
    path: &Path,
    fields: &BTreeMap<String, String>,
    key: &str,
) -> Result<String, SkillLoadError> {
    fields
        .get(key)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SkillLoadError::InvalidSkill {
            path: path.display().to_string(),
            message: format!("missing or empty `{key}` frontmatter field"),
        })
}

fn validate_len(
    path: &Path,
    field: &str,
    value: &str,
    max_chars: usize,
) -> Result<(), SkillLoadError> {
    if value.chars().count() > max_chars {
        return Err(SkillLoadError::InvalidSkill {
            path: path.display().to_string(),
            message: format!("`{field}` exceeds {max_chars} characters"),
        });
    }
    Ok(())
}

fn absolute_path(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}
