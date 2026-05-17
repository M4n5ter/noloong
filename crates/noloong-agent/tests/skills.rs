use noloong_agent::{
    AgentManifest, AgentPluginDeclaration, AgentSession, PluginComponent, PluginLoadFailurePolicy,
    SkillsPluginComponent,
};
use noloong_agent_core::{
    Agent, BoxFuture, CancellationToken, MessageRole, ModelProvider, ModelRequest,
    ModelStreamEvent, ModelStreamSink, StopReason,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[test]
fn skills_loader_discovers_direct_and_nested_skill_files() {
    let root = temp_dir("loader");
    write_skill(&root, "root-skill", "Root description");
    let nested = root.join("nested");
    fs::create_dir_all(&nested).unwrap();
    write_skill(&nested, "nested-skill", "Nested description");

    let component = SkillsPluginComponent {
        roots: vec![root.clone()],
    };
    let loaded =
        noloong_agent::skills::load_plugin_skills("skills-test", &component, Path::new("."))
            .unwrap();

    assert_eq!(loaded.skills.len(), 2);
    assert_eq!(loaded.skills[0].name, "nested-skill");
    assert_eq!(loaded.skills[1].name, "root-skill");
    assert!(loaded.skills.iter().all(|skill| skill.path.is_absolute()));
}

#[test]
fn skills_loader_rejects_missing_frontmatter_fields() {
    let root = temp_dir("invalid");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: broken\n---\nMissing description\n",
    )
    .unwrap();
    let component = SkillsPluginComponent { roots: vec![root] };

    let error =
        noloong_agent::skills::load_plugin_skills("skills-test", &component, Path::new("."))
            .unwrap_err();

    assert!(error.to_string().contains("description"));
}

#[test]
fn skills_metadata_renderer_uses_aliases_and_budget_warnings() {
    let root = temp_dir("render");
    let deep = root.join("very").join("long").join("path");
    fs::create_dir_all(&deep).unwrap();
    write_skill(&deep, "long-skill", &"description ".repeat(100));
    let component = SkillsPluginComponent {
        roots: vec![root.clone()],
    };
    let loaded =
        noloong_agent::skills::load_plugin_skills("skills-test", &component, Path::new("."))
            .unwrap();

    let rendered = noloong_agent::skills::render_skills_instructions(&loaded, 200)
        .expect("skills render should exist");

    assert!(rendered.text.contains("<skills_instructions>"));
    assert!(rendered.text.contains("skill-root-1"));
    assert!(!rendered.warnings.is_empty());
}

#[test]
fn explicit_skill_injection_accepts_structured_selection() {
    let root = temp_dir("structured");
    write_skill(&root, "structured-skill", "Structured skill description");
    let component = SkillsPluginComponent {
        roots: vec![root.clone()],
    };
    let loaded =
        noloong_agent::skills::load_plugin_skills("skills-test", &component, Path::new("."))
            .unwrap();
    let path = loaded.skills[0].path.display().to_string();
    let message = noloong_agent_core::AgentMessage {
        id: "user-1".into(),
        role: MessageRole::User,
        content: vec![noloong_agent_core::ContentBlock::Json {
            value: serde_json::json!({
                "type": "skill_selection",
                "path": path
            }),
        }],
        metadata: serde_json::Map::new(),
    };

    let rendered = noloong_agent::skills::render_explicit_skill_instructions(&loaded, &[message])
        .expect("structured skill selection should inject the skill body");

    assert!(rendered.text.contains("<skill_instructions>"));
    assert!(rendered.text.contains("Structured skill description"));
}

#[tokio::test]
async fn session_injects_skills_metadata_and_explicit_skill_body_request_locally() {
    let root = temp_dir("session");
    write_skill(&root, "echo-skill", "Echo skill description");
    let manifest = AgentManifest::default()
        .with_plugin(skills_plugin(root))
        .unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();
    let model = Arc::new(CapturingModelProvider::default());
    let runtime = session
        .runtime_builder()
        .with_model_provider(model.clone())
        .with_model_input_limit_tokens(100_000)
        .with_manifest_plugins()
        .await
        .unwrap()
        .build()
        .unwrap();
    let agent = Agent::builder()
        .with_runtime(Arc::new(runtime))
        .build()
        .unwrap();

    agent.prompt("Use $echo-skill now").await.unwrap();

    let requests = model.requests();
    let system_prompt = requests[0].messages.first().unwrap();
    assert_eq!(system_prompt.role, MessageRole::System);
    let text = message_text(system_prompt);
    assert!(text.contains("<skills_instructions>"));
    assert!(text.contains("echo-skill"));
    assert!(text.contains("<skill_instructions>"));
    assert!(text.contains("Echo skill description"));

    let state = agent.state().await;
    assert!(state.messages.iter().all(|message| {
        !message
            .content
            .iter()
            .any(|block| matches!(block, noloong_agent_core::ContentBlock::Text { text } if text.contains("<skill_instructions>")))
    }));
}

fn skills_plugin(root: PathBuf) -> AgentPluginDeclaration {
    AgentPluginDeclaration {
        plugin_id: "skills-test".into(),
        display_name: "Skills Test".into(),
        description: None,
        components: vec![PluginComponent::Skills(SkillsPluginComponent {
            roots: vec![root],
        })],
        enabled: true,
        on_load_failure: PluginLoadFailurePolicy::FailRun,
    }
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n"),
    )
    .unwrap();
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("noloong-skills-{name}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn message_text(message: &noloong_agent_core::AgentMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            noloong_agent_core::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Default)]
struct CapturingModelProvider {
    requests: Mutex<Vec<ModelRequest>>,
}

impl CapturingModelProvider {
    fn requests(&self) -> Vec<ModelRequest> {
        self.requests
            .lock()
            .expect("captured requests lock poisoned")
            .clone()
    }
}

impl ModelProvider for CapturingModelProvider {
    fn id(&self) -> &str {
        "capturing"
    }

    fn model_name(&self) -> Option<&str> {
        Some("gpt-5.4-mini")
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        _stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            self.requests
                .lock()
                .expect("captured requests lock poisoned")
                .push(request);
            Ok(vec![ModelStreamEvent::Finished {
                stop_reason: StopReason::Stop,
            }])
        })
    }
}
