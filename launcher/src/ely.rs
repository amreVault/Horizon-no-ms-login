//! Ely.by sign-in through the browser (docs/ely-by.md): the launcher opens Ely.by's authorization
//! page, the player signs in there, and Ely.by sends the browser back to a loopback address the
//! launcher listens on for that one request (`http://127.0.0.1:<port>/callback`). The launcher
//! then exchanges the code for tokens (OAuth2 authorization code grant, with the
//! `minecraft_server_session` scope, so the access token is a Minecraft session token for Ely.by's
//! session server), asks Ely.by who signed in, and keeps the tokens in the keychain.
//!
//! The player's password never reaches the launcher. The tokens are never logged or printed; the
//! game gets the access token in a session file that it reads once and deletes.
//!
//! Ely.by wants an application of its own (account.ely.by/dev/applications, type Website) with the
//! exact redirect address; its client id and secret are the player's own: `ely-setup` keeps them
//! in `ely-app.json`, or `HORIZON_ELY_CLIENT_ID` and `HORIZON_ELY_CLIENT_SECRET` set them.
use crate::account::{self, Accounts, SecretStore};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// What the sign-in asks for: who they are, a refresh token, and a Minecraft session token.
pub const SCOPES: &str = "account_info offline_access minecraft_server_session";
/// The loopback port the browser comes back to, unless the application says otherwise.
pub const DEFAULT_PORT: u16 = 38581;
/// How long the launcher waits for the player to finish in the browser.
const WAIT: Duration = Duration::from_secs(300);
/// A token this close to its end is renewed before the game starts.
const RENEW_BEFORE: u64 = 120;

/// Ely.by's addresses (tests point them at a mock).
#[derive(Clone, Debug)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub info: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://account.ely.by/oauth2/v1".into(),
            token: "https://account.ely.by/api/oauth2/v1/token".into(),
            info: "https://account.ely.by/api/account/v1/info".into(),
        }
    }
}

/// The player's Ely.by application.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub client_id: String,
    pub client_secret: String,
    /// The loopback port of the redirect address; 0 picks a free one (tests).
    #[serde(default = "default_port")]
    pub port: u16,
}
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<hidden>")
            .field("port", &self.port)
            .finish()
    }
}
fn default_port() -> u16 {
    DEFAULT_PORT
}

pub const NOT_CONFIGURED: &str = "Ely.by sign-in needs an application of your own: create one at https://account.ely.by/dev/applications (type Website, Redirect URI http://127.0.0.1:38581/callback), then run `horizon-launcher account ely-setup --client-id ID --client-secret SECRET` (docs/ely-by.md).";

impl Config {
    fn path() -> PathBuf {
        crate::backend::data_dir().join("ely-app.json")
    }

    /// The redirect address for `port`.
    pub fn redirect_uri(port: u16) -> String {
        format!("http://127.0.0.1:{port}/callback")
    }

    /// The application: the environment first, then `ely-app.json`, then what this build was
    /// compiled with.
    pub fn load() -> Result<Self, String> {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let port = env("HORIZON_ELY_PORT")
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(0);
        if let (Some(client_id), Some(client_secret)) =
            (env("HORIZON_ELY_CLIENT_ID"), env("HORIZON_ELY_CLIENT_SECRET"))
        {
            return Ok(Self {
                client_id,
                client_secret,
                port: if port == 0 { DEFAULT_PORT } else { port },
            });
        }
        if let Some(config) = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str::<Self>(&t).ok())
            .filter(|c| !c.client_id.is_empty() && !c.client_secret.is_empty())
        {
            return Ok(config);
        }
        match (
            option_env!("HORIZON_ELY_CLIENT_ID"),
            option_env!("HORIZON_ELY_CLIENT_SECRET"),
        ) {
            (Some(id), Some(secret)) if !id.is_empty() && !secret.is_empty() => Ok(Self {
                client_id: id.into(),
                client_secret: secret.into(),
                port: DEFAULT_PORT,
            }),
            _ => Err(NOT_CONFIGURED.into()),
        }
    }

    /// Keeps the application in `ely-app.json` (readable by its owner only, on Unix).
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_private(&path, text.as_bytes())
    }
}

/// Writes `bytes` to `path`, readable by its owner only on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("{}: {e}", path.display()))
}

// ----- The authorization page and the browser's way back ---------------------------------------

/// Percent-encodes a query value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(b));
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decodes a query value (`+` is a space).
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A random state, tying the browser's way back to this sign-in.
fn new_state() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("No randomness for the sign-in: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The page the player signs in on.
pub fn authorize_url(ep: &Endpoints, config: &Config, port: u16, state: &str) -> String {
    format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}",
        ep.authorize,
        encode(&config.client_id),
        encode(&Config::redirect_uri(port)),
        encode(SCOPES),
        encode(state),
    )
}

/// What the browser's request to the loopback address was.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    /// The authorization code, for this sign-in.
    Code(String),
    /// Ely.by or the player refused, or the state was not this sign-in's.
    Refused(String),
    /// Not the way back (a favicon, a probe).
    Other,
}

/// Reads the request line of the browser's request: `GET /callback?code=..&state=.. HTTP/1.1`.
pub fn parse_callback(request: &str, state: &str) -> Callback {
    let line = request.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::Other;
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/callback" {
        return Callback::Other;
    }
    let param = |name: &str| {
        query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| decode(v))
    };
    if let Some(error) = param("error") {
        return Callback::Refused(param("error_message").unwrap_or(error));
    }
    if param("state").as_deref() != Some(state) {
        return Callback::Refused("The sign-in did not start here: try again.".into());
    }
    match param("code").filter(|c| !c.is_empty()) {
        Some(code) => Callback::Code(code),
        None => Callback::Other,
    }
}

fn page(ok: bool, text: &str) -> String {
    let (title, body) = if ok {
        ("Signed in", "You can close this tab and go back to Horizon.")
    } else {
        ("Sign-in failed", text)
    };
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>Horizon: {title}</title>\
         <body style=\"font-family:sans-serif;margin:3em auto;max-width:30em\">\
         <h2>{title}</h2><p>{}</p>",
        body.replace('&', "&amp;").replace('<', "&lt;")
    );
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{html}",
        html.len()
    )
}

const NOT_FOUND: &str = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// Waits for the browser's way back on `listener`; the code, or why there is none.
pub fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    wait: Duration,
    cancel: &AtomicBool,
) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + wait;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Sign-in cancelled.".into());
        }
        if Instant::now() >= deadline {
            return Err("Sign-in timed out: the browser never came back.".into());
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let request = read_request(&mut stream);
        match parse_callback(&request, state) {
            Callback::Code(code) => {
                let _ = stream.write_all(page(true, "").as_bytes());
                return Ok(code);
            }
            Callback::Refused(why) => {
                let _ = stream.write_all(page(false, &why).as_bytes());
                return Err(why);
            }
            Callback::Other => {
                let _ = stream.write_all(NOT_FOUND.as_bytes());
            }
        }
    }
}

/// The head of the browser's request (a few KB at most).
fn read_request(stream: &mut TcpStream) -> String {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 8192 && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Opens `url` in the player's browser.
pub fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the browser ({e}): open the link yourself."))
}

// ----- Tokens -----------------------------------------------------------------------------------

/// What Ely.by's token endpoint gave, as the keychain keeps it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: u64,
}
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn agent(url: &str) -> ureq::Agent {
    crate::proxy::agent(
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .user_agent(&format!("horizon-launcher/{}", crate::backend::VERSION)),
        url,
    )
    .build()
}

/// Why a request to Ely.by failed, in words for the player.
fn failure(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let why = v
                .get("error_description")
                .or_else(|| v.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("no reason given");
            format!("Ely.by refused ({code}): {why}")
        }
        ureq::Error::Transport(_) => "Ely.by is unreachable: check the network.".into(),
    }
}

fn token_request(ep: &Endpoints, form: &[(&str, &str)]) -> Result<Tokens, String> {
    let response = agent(&ep.token).post(&ep.token).send_form(form).map_err(failure)?;
    let v: serde_json::Value = response
        .into_json()
        .map_err(|_| "Ely.by's answer was not understood.".to_string())?;
    let access_token = v
        .get("access_token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or("Ely.by gave no access token.")?
        .to_string();
    Ok(Tokens {
        access_token,
        refresh_token: v
            .get("refresh_token")
            .and_then(|t| t.as_str())
            .map(str::to_string),
        expires_at: now() + v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(3600),
    })
}

/// Exchanges the authorization code for tokens.
pub fn exchange(
    ep: &Endpoints,
    config: &Config,
    port: u16,
    code: &str,
) -> Result<Tokens, String> {
    token_request(
        ep,
        &[
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("redirect_uri", &Config::redirect_uri(port)),
            ("grant_type", "authorization_code"),
            ("code", code),
        ],
    )
}

/// New tokens from the refresh token; it doesn't expire, and the answer has none of its own.
pub fn refresh(ep: &Endpoints, config: &Config, old: &Tokens) -> Result<Tokens, String> {
    let refresh_token = old
        .refresh_token
        .as_deref()
        .ok_or("The sign-in has no refresh token: sign in again.")?;
    let mut tokens = token_request(
        ep,
        &[
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("scope", SCOPES),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )?;
    tokens.refresh_token = old.refresh_token.clone();
    Ok(tokens)
}

/// Who the token belongs to: the undashed UUID and the name.
pub fn user_info(ep: &Endpoints, access_token: &str) -> Result<(String, String), String> {
    let response = agent(&ep.info)
        .get(&ep.info)
        .set("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(failure)?;
    let v: serde_json::Value = response
        .into_json()
        .map_err(|_| "Ely.by's answer was not understood.".to_string())?;
    let uuid = v
        .get("uuid")
        .and_then(|u| u.as_str())
        .map(|u| u.replace('-', "").to_ascii_lowercase())
        .filter(|u| u.len() == 32 && u.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or("Ely.by gave no valid UUID.")?;
    let name = v
        .get("username")
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty() && n.chars().count() <= 16)
        .ok_or("Ely.by gave no valid player name.")?
        .to_string();
    Ok((uuid, name))
}

fn load_tokens(store: &dyn SecretStore, uuid: &str) -> Result<Tokens, String> {
    store
        .get(&account::ely_key(uuid))?
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or_else(|| "The Ely.by sign-in is gone from the keychain: sign in again.".into())
}

fn save_tokens(store: &dyn SecretStore, uuid: &str, tokens: &Tokens) -> Result<(), String> {
    let text = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
    store.set(&account::ely_key(uuid), &text)
}

// ----- Signing in -------------------------------------------------------------------------------

/// The whole browser round trip: `on_url` gets the page to open (the caller opens the browser, or
/// shows the link), then the launcher waits for the way back, exchanges the code and keeps the
/// tokens. Returns the account that signed in; it is not yet in `accounts.json`.
pub fn sign_in(
    ep: &Endpoints,
    config: &Config,
    store: &dyn SecretStore,
    on_url: &dyn Fn(&str),
    wait: Duration,
    cancel: &AtomicBool,
) -> Result<account::ElyAccount, String> {
    let listener = TcpListener::bind(("127.0.0.1", config.port)).map_err(|e| {
        format!(
            "Port {} is busy ({e}): close what uses it, or set another port in the application's \
             Redirect URI and `ely-setup --port`.",
            config.port
        )
    })?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = new_state()?;
    on_url(&authorize_url(ep, config, port, &state));
    let code = wait_for_code(&listener, &state, wait, cancel)?;
    drop(listener);
    let tokens = exchange(ep, config, port, &code)?;
    let (uuid, name) = user_info(ep, &tokens.access_token)?;
    save_tokens(store, &uuid, &tokens)?;
    Ok(account::ElyAccount { uuid, name })
}

/// Signs in and keeps the account: it is added to `accounts.json` and plays from now on.
pub fn sign_in_and_keep(
    ep: &Endpoints,
    config: &Config,
    store: &dyn SecretStore,
    on_url: &dyn Fn(&str),
    cancel: &AtomicBool,
) -> Result<String, String> {
    let account = sign_in(ep, config, store, on_url, WAIT, cancel)?;
    let mut accounts = Accounts::load();
    accounts.add_ely(&account.uuid, &account.name);
    accounts.save()?;
    Ok(account.name)
}

/// Where a sign-in under way is, for a window or a tool that shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInState {
    Starting,
    /// Waiting for the browser; the page to open if it didn't.
    Waiting(String),
    /// The account's name, or why it failed.
    Done(Result<String, String>),
}

/// A sign-in running on its own thread.
pub struct SignIn {
    state: Arc<Mutex<SignInState>>,
    cancel: Arc<AtomicBool>,
}

impl SignIn {
    /// Opens the browser and waits for the player in the background.
    pub fn start() -> Self {
        let state = Arc::new(Mutex::new(SignInState::Starting));
        let cancel = Arc::new(AtomicBool::new(false));
        let (s, c) = (state.clone(), cancel.clone());
        std::thread::spawn(move || {
            let done = Config::load().and_then(|config| {
                sign_in_and_keep(
                    &Endpoints::default(),
                    &config,
                    account::store(),
                    &|url| {
                        *s.lock().unwrap() = SignInState::Waiting(url.to_string());
                        let _ = open_browser(url);
                    },
                    &c,
                )
            });
            *s.lock().unwrap() = SignInState::Done(done);
        });
        Self { state, cancel }
    }
    pub fn state(&self) -> SignInState {
        self.state.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

// ----- Playing ----------------------------------------------------------------------------------

/// The session file the game reads once and deletes: for the Ely.by account that plays, with its
/// token renewed first when it is about to end. `None` when no Ely.by account plays. An error is
/// not the end: the caller starts the game offline as that account's name.
pub fn session_for_game(
    accounts: &Accounts,
    store: &dyn SecretStore,
    ep: &Endpoints,
    config: Option<&Config>,
    dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let account::Player::Ely(uuid) = accounts.player() else {
        return Ok(None);
    };
    let Some(account) = accounts.ely.iter().find(|a| a.uuid == uuid) else {
        return Ok(None);
    };
    let mut tokens = load_tokens(store, &uuid)?;
    if tokens.expires_at <= now() + RENEW_BEFORE {
        let config = config.ok_or(NOT_CONFIGURED)?;
        tokens = refresh(ep, config, &tokens)?;
        save_tokens(store, &uuid, &tokens)?;
    }
    write_session_file(dir, &account.name, &uuid, &tokens.access_token).map(Some)
}

/// How the game starts for whoever plays.
#[derive(Debug, Default)]
pub struct Start {
    /// The Ely.by session file (`--session-file`).
    pub session: Option<PathBuf>,
    /// The offline name (`--name`).
    pub name: Option<String>,
    /// Why an Ely.by account plays offline, when its session could not be made.
    pub notice: Option<String>,
}

/// Starts for whoever plays: an offline name, the game's own name, or an Ely.by session (which
/// falls back to that account's name, offline, when the session can't be made).
pub fn start_for(
    accounts: &Accounts,
    store: &dyn SecretStore,
    ep: &Endpoints,
    config: Option<&Config>,
    dir: &Path,
) -> Start {
    match session_for_game(accounts, store, ep, config, dir) {
        Ok(Some(file)) => Start {
            session: Some(file),
            ..Start::default()
        },
        Ok(None) => Start {
            name: accounts.name_arg().map(str::to_string),
            ..Start::default()
        },
        Err(e) => Start {
            name: match accounts.player() {
                account::Player::Ely(uuid) => accounts
                    .ely
                    .iter()
                    .find(|a| a.uuid == uuid)
                    .map(|a| a.name.clone()),
                _ => None,
            },
            notice: Some(format!("Playing offline: {e}")),
            ..Start::default()
        },
    }
}

/// `start_for` with the launcher's own keychain, application and game folder.
pub fn start_for_player(accounts: &Accounts) -> Start {
    start_for(
        accounts,
        account::store(),
        &Endpoints::default(),
        Config::load().ok().as_ref(),
        &crate::backend::game_home(),
    )
}

/// Adds how the game starts to its command.
pub fn apply(cmd: &mut std::process::Command, start: &Start) {
    if let Some(file) = &start.session {
        cmd.arg("--session-file").arg(file);
    } else if let Some(name) = &start.name {
        cmd.arg("--name").arg(name);
    }
}

/// The session file's JSON, as the game reads it (`user_type` "ely").
pub fn session_json(name: &str, uuid: &str, access_token: &str) -> String {
    serde_json::json!({
        "name": name,
        "uuid": uuid,
        "access_token": access_token,
        "user_type": "ely",
    })
    .to_string()
}

fn write_session_file(
    dir: &Path,
    name: &str,
    uuid: &str,
    access_token: &str,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let path = dir.join(format!(
        ".horizon-session-{}.json",
        random.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    write_private(&path, session_json(name, uuid, access_token).as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests;
