#![cfg(feature = "registry-store-postgres")]

use noloong_agent::{
    AgentManifest,
    interaction::{
        AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentSessionListFilter, AgentSessionRecord,
        AgentSessionRegistryStore, SqlAgentSessionRegistryStore,
        SqlAgentSessionRegistryStoreConfig,
    },
};
use noloong_agent_core::{AgentMessage, AgentState, RunStatus};
use serde_json::{Map, json};
use std::time::{SystemTime, UNIX_EPOCH};

const POSTGRES_URL_ENV: &str = "NOLOONG_POSTGRES_TEST_URL";

#[tokio::test]
async fn postgres_store_round_trips_when_url_is_available() {
    let Ok(database_url) = std::env::var(POSTGRES_URL_ENV) else {
        init_test_logger();
        log::info!("skipping PostgreSQL registry store test; {POSTGRES_URL_ENV} is not set");
        return;
    };
    let store = SqlAgentSessionRegistryStore::connect(
        SqlAgentSessionRegistryStoreConfig::new(database_url)
            .with_table_name_prefix(unique_table_prefix()),
    )
    .await
    .unwrap();

    store.insert(record("root")).await.unwrap();
    let mut stored = store.get("root").await.unwrap().unwrap();
    stored.metadata.insert("backend".into(), json!("postgres"));
    store.save(stored).await.unwrap();

    let listed = store
        .list(&AgentSessionListFilter::default())
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].metadata["backend"], "postgres");

    store.remove("root").await.unwrap();
    assert!(store.get("root").await.unwrap().is_none());
}

fn init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();
}

fn record(session_id: &str) -> AgentSessionRecord {
    AgentSessionRecord {
        schema_version: AGENT_SESSION_RECORD_SCHEMA_VERSION,
        session_id: session_id.into(),
        profile_id: "default".into(),
        parent_session_id: None,
        role: None,
        run_id_prefix: format!("stored-{session_id}"),
        manifest: AgentManifest::default(),
        state: AgentState {
            run_id: Some("run-1".into()),
            status: RunStatus::Completed,
            messages: vec![AgentMessage::user("stored-user", "hello")],
            completed_turns: 1,
            ..AgentState::default()
        },
        queues: Default::default(),
        metadata: Map::new(),
        created_at_ms: 1,
        updated_at_ms: 2,
    }
}

fn unique_table_prefix() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("noloong_registry_test_{}_{}_", std::process::id(), now)
}
