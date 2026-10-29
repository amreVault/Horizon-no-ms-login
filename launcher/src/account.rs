//! Who plays. This fork has no Microsoft sign-in: the launcher never contacts Microsoft, Xbox or
//! the Minecraft services. The game starts offline with a name the player picks (`--name`) or the
//! name the game made up (`horizon-player.txt`), or as an Ely.by account (`ely`, signed in through
//! the browser).
//!
//! `accounts.json` keeps the offline names, the Ely.by accounts (names and UUIDs only; their
//! tokens live in the keychain), which one plays and each offline name's skin and cape look.
//! The keychain helpers also keep the launcher's other secrets (the Tailscale token).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The keychain service name every secret is stored under.
pub const SERVICE: &str = "dev.playhorizon.launcher";

// ----- The keychain --------------------------------------------------------------------------------

/// Where secrets are kept: the OS keychain, or memory in tests and when there is none.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
    /// False for memory: secrets are gone when the launcher closes.
    fn persistent(&self) -> bool;
}

#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(key.into(), secret.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
    fn persistent(&self) -> bool {
        false
    }
}

/// The Secret Service (GNOME Keyring, KWallet) through oo7, as gpui itself uses it.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub struct SystemStore;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl SystemStore {
    fn keyring() -> Result<oo7::Keyring, String> {
        futures_lite::future::block_on(async {
            let keyring = oo7::Keyring::new().await.map_err(|e| e.to_string())?;
            keyring.unlock().await.map_err(|e| e.to_string())?;
            Ok(keyring)
        })
    }
}
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl SecretStore for SystemStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            let items = keyring
                .search_items(&attributes)
                .await
                .map_err(|e| e.to_string())?;
            let Some(item) = items.into_iter().next() else {
                return Ok(None);
            };
            item.unlock().await.map_err(|e| e.to_string())?;
            let secret = item.secret().await.map_err(|e| e.to_string())?;
            Ok(String::from_utf8(secret.to_vec()).ok())
        })
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            keyring
                .create_item(
                    "Horizon launcher: Minecraft account",
                    &attributes,
                    secret.as_bytes(),
                    true,
                )
                .await
                .map_err(|e| e.to_string())
        })
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            keyring.delete(&attributes).await.map_err(|e| e.to_string())
        })
    }
    fn persistent(&self) -> bool {
        true
    }
}

/// Keychain on macOS, Credential Manager on Windows (the keyring crate). Credential Manager takes at
/// most 2560 bytes of UTF-16 per item and a sign-in's secrets are longer, so a secret is kept in
/// pieces: the first under the account's key, the rest under `key.1`, `key.2`...
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct SystemStore;

/// Bytes of UTF-8 per keychain item: at most twice as many in UTF-16, well under 2560.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const PIECE: usize = 1000;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn piece_key(key: &str, n: usize) -> String {
    if n == 0 {
        key.to_string()
    } else {
        format!("{key}.{n}")
    }
}

/// `secret` cut into pieces of at most `PIECE` bytes, on character boundaries.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn split_secret(mut secret: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    while secret.len() > PIECE {
        let mut cut = PIECE;
        while !secret.is_char_boundary(cut) {
            cut -= 1;
        }
        let (head, tail) = secret.split_at(cut);
        pieces.push(head);
        secret = tail;
    }
    pieces.push(secret);
    pieces
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn read_item(key: &str) -> Result<Option<String>, String> {
    match keyring::Entry::new(SERVICE, key).and_then(|e| e.get_password()) {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn write_item(key: &str, secret: &str) -> Result<(), String> {
    keyring::Entry::new(SERVICE, key)
        .and_then(|e| e.set_password(secret))
        .map_err(|e| e.to_string())
}

/// Removes one item; false when there was none.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn remove_item(key: &str) -> Result<bool, String> {
    match keyring::Entry::new(SERVICE, key).and_then(|e| e.delete_credential()) {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SecretStore for SystemStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let Some(mut secret) = read_item(key)? else {
            return Ok(None);
        };
        for n in 1.. {
            match read_item(&piece_key(key, n))? {
                Some(piece) => secret.push_str(&piece),
                None => break,
            }
        }
        Ok(Some(secret))
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        let pieces = split_secret(secret);
        for (n, piece) in pieces.iter().enumerate() {
            write_item(&piece_key(key, n), piece)?;
        }
        // Pieces left over from a longer secret would be read back as part of this one.
        for n in pieces.len().. {
            if !remove_item(&piece_key(key, n))? {
                break;
            }
        }
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        for n in 0.. {
            if !remove_item(&piece_key(key, n))? {
                break;
            }
        }
        Ok(())
    }
    fn persistent(&self) -> bool {
        true
    }
}

pub const NO_KEYCHAIN: &str = "No keychain available: saved secrets last only until the launcher closes.";

/// The keychain if it answers, else memory for this run (said once in the UI with NO_KEYCHAIN).
pub fn store() -> &'static dyn SecretStore {
    static STORE: OnceLock<Box<dyn SecretStore>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            // Unit tests never reach the player's keychain.
            #[cfg(all(
                not(test),
                any(
                    target_os = "linux",
                    target_os = "freebsd",
                    target_os = "macos",
                    target_os = "windows"
                )
            ))]
            if std::env::var_os("HORIZON_NO_KEYCHAIN").is_none() && SystemStore.get("probe").is_ok()
            {
                return Box::new(SystemStore);
            }
            Box::<MemoryStore>::default()
        })
        .as_ref()
}


// ----- Offline players -----------------------------------------------------------------------------

/// accounts.json: the offline names the player keeps and the one that plays.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Accounts {
    /// Ely.by accounts signed in through the browser (`ely`): names and UUIDs; the tokens are in
    /// the keychain under `ely_key`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ely: Vec<ElyAccount>,
    /// The Ely.by account that plays (its UUID, undashed); none: an offline name plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ely_selected: Option<String>,
    /// Offline names the player keeps, to switch between (`--name`): no account.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offline: Vec<String>,
    /// The offline name that plays; none: the name the game made up (`horizon-player.txt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_selected: Option<String>,
    /// Each offline name's skin and cape (`skins`), by the name in lowercase.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub looks: std::collections::BTreeMap<String, crate::skins::Look>,
}

/// An Ely.by account the launcher is signed in to: no secrets here.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ElyAccount {
    /// The undashed lowercase UUID.
    pub uuid: String,
    pub name: String,
}

/// The keychain key an Ely.by account's tokens are kept under.
pub fn ely_key(uuid: &str) -> String {
    format!("ely:{uuid}")
}

/// Who plays, as the quick switch lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Player {
    /// An Ely.by account (its UUID).
    Ely(String),
    /// An offline name.
    Offline(String),
    /// Offline as the name the game made up.
    GameName,
}

/// Whether `name` can be an offline name: 3 to 16 letters, digits or `_`.
pub fn valid_offline_name(name: &str) -> bool {
    (3..=16).contains(&name.chars().count())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Accounts {
    fn path() -> PathBuf {
        crate::backend::data_dir().join("accounts.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| e.to_string())
    }

    /// Who plays now.
    pub fn player(&self) -> Player {
        if let Some(uuid) = &self.ely_selected {
            if self.ely.iter().any(|a| &a.uuid == uuid) {
                return Player::Ely(uuid.clone());
            }
        }
        match &self.offline_selected {
            Some(name) => Player::Offline(name.clone()),
            None => Player::GameName,
        }
    }

    /// Plays as `who`: an Ely.by account, an offline name (kept) or the game's own name.
    pub fn play_as(&mut self, who: Player) -> Result<(), String> {
        match who {
            Player::Ely(uuid) => {
                if !self.ely.iter().any(|a| a.uuid == uuid) {
                    return Err("That Ely.by account is not signed in.".into());
                }
                self.ely_selected = Some(uuid);
            }
            Player::Offline(name) => {
                let name = self.add_offline(&name)?;
                self.offline_selected = Some(name);
                self.ely_selected = None;
            }
            Player::GameName => {
                self.offline_selected = None;
                self.ely_selected = None;
            }
        }
        Ok(())
    }

    /// Keeps an Ely.by account (its name follows renames) and plays as it.
    pub fn add_ely(&mut self, uuid: &str, name: &str) {
        let uuid = uuid.replace('-', "").to_ascii_lowercase();
        match self.ely.iter_mut().find(|a| a.uuid == uuid) {
            Some(kept) => kept.name = name.to_string(),
            None => self.ely.push(ElyAccount {
                uuid: uuid.clone(),
                name: name.to_string(),
            }),
        }
        self.ely_selected = Some(uuid);
    }

    /// The Ely.by account called `who` (name in any case, or UUID).
    pub fn find_ely(&self, who: &str) -> Option<&ElyAccount> {
        let bare = who.replace('-', "").to_ascii_lowercase();
        self.ely
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(who) || a.uuid == bare)
    }

    /// Signs an Ely.by account out: its tokens leave the keychain; if it was playing, the game's
    /// own name plays.
    pub fn remove_ely(&mut self, uuid: &str, store: &dyn SecretStore) -> Result<(), String> {
        let before = self.ely.len();
        self.ely.retain(|a| a.uuid != uuid);
        if self.ely.len() == before {
            return Err("That Ely.by account is not signed in.".into());
        }
        store.delete(&ely_key(uuid))?;
        if self.ely_selected.as_deref() == Some(uuid) {
            self.ely_selected = None;
        }
        Ok(())
    }

    /// Keeps an offline name (once, in any case); returns it as kept.
    fn add_offline(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if !valid_offline_name(name) {
            return Err(format!(
                "{name}: an offline name is 3 to 16 letters, digits or _"
            ));
        }
        if let Some(kept) = self.offline.iter().find(|n| n.eq_ignore_ascii_case(name)) {
            return Ok(kept.clone());
        }
        self.offline.push(name.to_string());
        Ok(name.to_string())
    }

    /// Forgets an offline name: its look and its skin and cape files (`skins`) go with it; if
    /// it was playing, the game's own name plays.
    pub fn remove_offline(&mut self, name: &str, skins_dir: &Path) -> Result<(), String> {
        let before = self.offline.len();
        self.offline.retain(|n| !n.eq_ignore_ascii_case(name));
        self.looks.remove(&name.to_ascii_lowercase());
        if self.offline.len() == before {
            return Err(format!("No offline name {name}."));
        }
        crate::skins::forget(skins_dir, name);
        if self
            .offline_selected
            .as_ref()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
        {
            self.offline_selected = None;
        }
        Ok(())
    }

    /// An offline name's look (the default: no skin of its own).
    pub fn look(&self, name: &str) -> crate::skins::Look {
        self.looks
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// The game's variables for the offline name that plays: its skin and cape.
    pub fn look_env(&self) -> Vec<(&'static str, String)> {
        match self.name_arg() {
            Some(name) => crate::skins::env(&crate::skins::dir(), name, &self.look(name)),
            None => Vec::new(),
        }
    }

    /// The game's `--name` for this run: the offline name, when one plays.
    pub fn name_arg(&self) -> Option<&str> {
        match self.player() {
            Player::Offline(_) => self.offline_selected.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
