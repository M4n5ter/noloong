use crate::{OpenAiIntegrationError, Result};
use std::{
    fmt::{Debug, Formatter},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::ChatGptTokenData;

pub trait ChatGptTokenStore: Send + Sync {
    fn load(&self) -> Result<Option<ChatGptTokenData>>;

    fn save(&self, token: &ChatGptTokenData) -> Result<()>;

    fn delete(&self) -> Result<()>;
}

pub trait ChatGptKeyring: Send + Sync {
    fn get_password(&self, service: &str, account: &str) -> Result<Option<String>>;

    fn set_password(&self, service: &str, account: &str, password: &str) -> Result<()>;

    fn delete_password(&self, service: &str, account: &str) -> Result<()>;
}

#[derive(Clone, Debug)]
pub enum ChatGptTokenStorage {
    File(ChatGptFileTokenStorage),
    Keyring(ChatGptKeyringTokenStorage),
    Auto(ChatGptAutoTokenStorage),
    Ephemeral(ChatGptEphemeralTokenStorage),
}

impl ChatGptTokenStorage {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(ChatGptFileTokenStorage::new(path))
    }

    pub fn keyring(
        service: impl Into<String>,
        account: impl Into<String>,
        keyring: Arc<dyn ChatGptKeyring>,
    ) -> Self {
        Self::Keyring(ChatGptKeyringTokenStorage::new(service, account, keyring))
    }

    pub fn auto(keyring: ChatGptKeyringTokenStorage, file: ChatGptFileTokenStorage) -> Self {
        Self::Auto(ChatGptAutoTokenStorage::new(keyring, file))
    }

    pub fn ephemeral() -> Self {
        Self::Ephemeral(ChatGptEphemeralTokenStorage::new())
    }
}

impl ChatGptTokenStore for ChatGptTokenStorage {
    fn load(&self) -> Result<Option<ChatGptTokenData>> {
        match self {
            Self::File(storage) => storage.load(),
            Self::Keyring(storage) => storage.load(),
            Self::Auto(storage) => storage.load(),
            Self::Ephemeral(storage) => storage.load(),
        }
    }

    fn save(&self, token: &ChatGptTokenData) -> Result<()> {
        match self {
            Self::File(storage) => storage.save(token),
            Self::Keyring(storage) => storage.save(token),
            Self::Auto(storage) => storage.save(token),
            Self::Ephemeral(storage) => storage.save(token),
        }
    }

    fn delete(&self) -> Result<()> {
        match self {
            Self::File(storage) => storage.delete(),
            Self::Keyring(storage) => storage.delete(),
            Self::Auto(storage) => storage.delete(),
            Self::Ephemeral(storage) => storage.delete(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatGptFileTokenStorage {
    path: PathBuf,
}

impl ChatGptFileTokenStorage {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ChatGptTokenStore for ChatGptFileTokenStorage {
    fn load(&self) -> Result<Option<ChatGptTokenData>> {
        match fs::read_to_string(&self.path) {
            Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, token: &ChatGptTokenData) -> Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_vec_pretty(token)?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path)?;
        file.write_all(&content)?;
        file.write_all(b"\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Clone)]
pub struct ChatGptKeyringTokenStorage {
    service: String,
    account: String,
    keyring: Arc<dyn ChatGptKeyring>,
}

impl ChatGptKeyringTokenStorage {
    pub fn new(
        service: impl Into<String>,
        account: impl Into<String>,
        keyring: Arc<dyn ChatGptKeyring>,
    ) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
            keyring,
        }
    }
}

impl Debug for ChatGptKeyringTokenStorage {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatGptKeyringTokenStorage")
            .field("service", &self.service)
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl ChatGptTokenStore for ChatGptKeyringTokenStorage {
    fn load(&self) -> Result<Option<ChatGptTokenData>> {
        self.keyring
            .get_password(&self.service, &self.account)?
            .map(|secret| serde_json::from_str(&secret).map_err(Into::into))
            .transpose()
    }

    fn save(&self, token: &ChatGptTokenData) -> Result<()> {
        let secret = serde_json::to_string(token)?;
        self.keyring
            .set_password(&self.service, &self.account, &secret)
    }

    fn delete(&self) -> Result<()> {
        self.keyring.delete_password(&self.service, &self.account)
    }
}

#[derive(Clone, Debug)]
pub struct ChatGptAutoTokenStorage {
    keyring: ChatGptKeyringTokenStorage,
    file: ChatGptFileTokenStorage,
}

impl ChatGptAutoTokenStorage {
    pub fn new(keyring: ChatGptKeyringTokenStorage, file: ChatGptFileTokenStorage) -> Self {
        Self { keyring, file }
    }
}

impl ChatGptTokenStore for ChatGptAutoTokenStorage {
    fn load(&self) -> Result<Option<ChatGptTokenData>> {
        match self.keyring.load() {
            Ok(Some(token)) => Ok(Some(token)),
            Ok(None) | Err(_) => self.file.load(),
        }
    }

    fn save(&self, token: &ChatGptTokenData) -> Result<()> {
        self.keyring.save(token).or_else(|_| self.file.save(token))
    }

    fn delete(&self) -> Result<()> {
        let keyring_result = self.keyring.delete();
        let file_result = self.file.delete();
        keyring_result.or(file_result)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ChatGptEphemeralTokenStorage {
    token: Arc<Mutex<Option<ChatGptTokenData>>>,
}

impl ChatGptEphemeralTokenStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ChatGptTokenStore for ChatGptEphemeralTokenStorage {
    fn load(&self) -> Result<Option<ChatGptTokenData>> {
        Ok(self
            .token
            .lock()
            .map_err(|_| OpenAiIntegrationError::Storage("ephemeral token lock poisoned".into()))?
            .clone())
    }

    fn save(&self, token: &ChatGptTokenData) -> Result<()> {
        *self.token.lock().map_err(|_| {
            OpenAiIntegrationError::Storage("ephemeral token lock poisoned".into())
        })? = Some(token.clone());
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        self.token
            .lock()
            .map_err(|_| OpenAiIntegrationError::Storage("ephemeral token lock poisoned".into()))?
            .take();
        Ok(())
    }
}
