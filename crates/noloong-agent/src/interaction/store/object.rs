use super::codec::{decode_record_json, encode_record_json};
use super::{
    AgentSessionRecord, AgentSessionRegistryStore, duplicate_session_error, missing_session_error,
};
use crate::interaction::{InteractionError, InteractionFuture};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use opendal::{ErrorKind, Operator};

#[derive(Clone, Debug, Default)]
pub struct OpenDalAgentSessionRegistryStoreConfig {
    pub prefix: String,
}

impl OpenDalAgentSessionRegistryStoreConfig {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }
}

#[derive(Clone)]
pub struct OpenDalAgentSessionRegistryStore {
    operator: Operator,
    prefix: String,
}

impl OpenDalAgentSessionRegistryStore {
    pub fn new(operator: Operator, config: OpenDalAgentSessionRegistryStoreConfig) -> Self {
        Self {
            operator,
            prefix: normalize_prefix(&config.prefix),
        }
    }

    fn session_path(&self, session_id: &str) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(session_id.as_bytes());
        format!("{}{encoded}.json", self.prefix)
    }
}

impl AgentSessionRegistryStore for OpenDalAgentSessionRegistryStore {
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.session_path(&record.session_id);
            if self.operator.exists(&path).await.map_err(to_store_error)? {
                return Err(duplicate_session_error(&record.session_id));
            }
            let bytes = encode_record_json(&record)?.into_bytes();
            self.operator
                .write(&path, bytes)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.session_path(&record.session_id);
            if !self.operator.exists(&path).await.map_err(to_store_error)? {
                return Err(missing_session_error(&record.session_id));
            }
            let bytes = encode_record_json(&record)?.into_bytes();
            self.operator
                .write(&path, bytes)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.session_path(session_id);
            self.operator.delete(&path).await.map_err(to_store_error)?;
            Ok(())
        })
    }

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>> {
        Box::pin(async move {
            let path = self.session_path(session_id);
            let bytes = match self.operator.read(&path).await {
                Ok(bytes) => bytes.to_bytes(),
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(to_store_error(error)),
            };
            Ok(Some(decode_record_json(session_id, bytes.as_ref())?))
        })
    }

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        Box::pin(async move {
            let entries = self
                .operator
                .list(&self.prefix)
                .await
                .map_err(to_store_error)?;
            let mut records = Vec::new();
            for entry in entries {
                if !entry.path().ends_with(".json") {
                    continue;
                }
                let bytes = self
                    .operator
                    .read(entry.path())
                    .await
                    .map_err(to_store_error)?
                    .to_bytes();
                records.push(decode_record_json(entry.path(), bytes.as_ref())?);
            }
            records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
            Ok(records)
        })
    }
}

fn normalize_prefix(prefix: &str) -> String {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    }
}

fn to_store_error(error: impl std::fmt::Display) -> InteractionError {
    InteractionError::internal(format!(
        "opendal agent session registry store error: {error}"
    ))
}
