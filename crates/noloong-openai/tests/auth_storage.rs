use noloong_openai::auth::{
    ChatGptAutoTokenStorage, ChatGptFileTokenStorage, ChatGptKeyring, ChatGptKeyringTokenStorage,
    ChatGptTokenData, ChatGptTokenStore,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn auth_file_storage_saves_loads_deletes_token() -> noloong_openai::Result<()> {
    let path = unique_temp_path("file-storage-token.json");
    let storage = ChatGptFileTokenStorage::new(&path);
    let token = sample_token();

    storage.save(&token)?;

    assert_eq!(storage.load()?, Some(token));
    assert_file_mode_is_private(&path);

    storage.delete()?;

    assert_eq!(storage.load()?, None);
    Ok(())
}

#[test]
fn auth_keyring_storage_saves_loads_deletes_token() -> noloong_openai::Result<()> {
    let keyring = Arc::new(MemoryKeyring::default());
    let storage = ChatGptKeyringTokenStorage::new("noloong", "chatgpt", keyring);
    let token = sample_token();

    storage.save(&token)?;

    assert_eq!(storage.load()?, Some(token));

    storage.delete()?;

    assert_eq!(storage.load()?, None);
    Ok(())
}

#[test]
fn auth_auto_storage_falls_back_to_file_when_keyring_fails() -> noloong_openai::Result<()> {
    let path = unique_temp_path("auto-storage-token.json");
    let keyring = ChatGptKeyringTokenStorage::new("noloong", "chatgpt", Arc::new(FailingKeyring));
    let file = ChatGptFileTokenStorage::new(&path);
    let storage = ChatGptAutoTokenStorage::new(keyring, file);
    let token = sample_token();

    storage.save(&token)?;

    assert_eq!(storage.load()?, Some(token));
    Ok(())
}

#[test]
fn auth_ephemeral_storage_saves_loads_deletes_token() -> noloong_openai::Result<()> {
    let storage = noloong_openai::auth::ChatGptEphemeralTokenStorage::new();
    let token = sample_token();

    storage.save(&token)?;
    assert_eq!(storage.load()?, Some(token));

    storage.delete()?;
    assert_eq!(storage.load()?, None);
    Ok(())
}

fn sample_token() -> ChatGptTokenData {
    ChatGptTokenData::new("id-token", "access-token", "refresh-token", 123)
        .account_id("account-123")
}

fn unique_temp_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("noloong-openai-{unique}-{name}"))
}

#[cfg(unix)]
fn assert_file_mode_is_private(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mode = std::fs::metadata(path)
        .expect("token file metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(not(unix))]
fn assert_file_mode_is_private(_path: &std::path::Path) {}

#[derive(Default)]
struct MemoryKeyring {
    secrets: Mutex<BTreeMap<(String, String), String>>,
}

impl ChatGptKeyring for MemoryKeyring {
    fn get_password(&self, service: &str, account: &str) -> noloong_openai::Result<Option<String>> {
        Ok(self
            .secrets
            .lock()
            .expect("memory keyring lock")
            .get(&(service.into(), account.into()))
            .cloned())
    }

    fn set_password(
        &self,
        service: &str,
        account: &str,
        password: &str,
    ) -> noloong_openai::Result<()> {
        self.secrets
            .lock()
            .expect("memory keyring lock")
            .insert((service.into(), account.into()), password.into());
        Ok(())
    }

    fn delete_password(&self, service: &str, account: &str) -> noloong_openai::Result<()> {
        self.secrets
            .lock()
            .expect("memory keyring lock")
            .remove(&(service.into(), account.into()));
        Ok(())
    }
}

struct FailingKeyring;

impl ChatGptKeyring for FailingKeyring {
    fn get_password(
        &self,
        _service: &str,
        _account: &str,
    ) -> noloong_openai::Result<Option<String>> {
        Err(noloong_openai::OpenAiIntegrationError::Storage(
            "keyring unavailable".into(),
        ))
    }

    fn set_password(
        &self,
        _service: &str,
        _account: &str,
        _password: &str,
    ) -> noloong_openai::Result<()> {
        Err(noloong_openai::OpenAiIntegrationError::Storage(
            "keyring unavailable".into(),
        ))
    }

    fn delete_password(&self, _service: &str, _account: &str) -> noloong_openai::Result<()> {
        Err(noloong_openai::OpenAiIntegrationError::Storage(
            "keyring unavailable".into(),
        ))
    }
}
