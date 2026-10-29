use super::*;
use crate::account::{MemoryStore, Player};

const UUID: &str = "ffc8fdc95824509e8a57c99b940fb996";

#[test]
fn query_values_round_trip() {
    assert_eq!(encode("a b/c:d"), "a%20b%2Fc%3Ad");
    assert_eq!(decode("a%20b%2Fc%3Ad+e"), "a b/c:d e");
    assert_eq!(decode("100%"), "100%");
    assert_eq!(decode("%zz%4"), "%zz%4");
    assert_eq!(decode(&encode("héllo wörld&=")), "héllo wörld&=");
}

#[test]
fn the_authorization_page_asks_for_the_session_scope() {
    let config = Config {
        client_id: "my-app".into(),
        client_secret: "shh".into(),
        port: DEFAULT_PORT,
    };
    let url = authorize_url(&Endpoints::default(), &config, 38581, "st4te");
    assert!(url.starts_with("https://account.ely.by/oauth2/v1?client_id=my-app&"));
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A38581%2Fcallback"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("scope=account_info%20offline_access%20minecraft_server_session"));
    assert!(url.ends_with("&state=st4te"));
    assert!(!url.contains("shh"), "the secret never goes in the browser's address");
}

#[test]
fn the_browsers_way_back_is_read() {
    let get = |target: &str| format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert_eq!(
        parse_callback(&get("/callback?code=abc%2B1&state=s"), "s"),
        Callback::Code("abc+1".into())
    );
    // Another sign-in's state is refused, never exchanged.
    assert!(matches!(
        parse_callback(&get("/callback?code=abc&state=other"), "s"),
        Callback::Refused(_)
    ));
    assert!(matches!(
        parse_callback(&get("/callback?code=abc"), "s"),
        Callback::Refused(_)
    ));
    assert_eq!(
        parse_callback(
            &get("/callback?error=access_denied&error_message=The+player+said+no&state=s"),
            "s"
        ),
        Callback::Refused("The player said no".into())
    );
    assert_eq!(parse_callback(&get("/favicon.ico"), "s"), Callback::Other);
    assert_eq!(parse_callback(&get("/callback?state=s"), "s"), Callback::Other);
    assert_eq!(
        parse_callback("POST /callback?code=a&state=s HTTP/1.1\r\n\r\n", "s"),
        Callback::Other
    );
    assert_eq!(parse_callback("", "s"), Callback::Other);
}

/// A stand-in for Ely.by: the token endpoint and the account info, logging what it was asked.
struct Mock {
    port: u16,
    log: Arc<Mutex<Vec<String>>>,
}
impl Mock {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let shared = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                serve(&mut stream, &shared);
            }
        });
        Self { port, log }
    }
    fn endpoints(&self) -> Endpoints {
        let base = format!("http://127.0.0.1:{}", self.port);
        Endpoints {
            authorize: format!("{base}/oauth2/v1"),
            token: format!("{base}/token"),
            info: format!("{base}/info"),
        }
    }
    fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

fn serve(stream: &mut TcpStream, log: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let (mut head_end, mut length) = (None, 0usize);
    loop {
        if head_end.is_none() {
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                head_end = Some(at + 4);
                let head = String::from_utf8_lossy(&buf[..at]).to_ascii_lowercase();
                length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
            }
        }
        if head_end.is_some_and(|h| buf.len() >= h + length) {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let request = String::from_utf8_lossy(&buf).into_owned();
    log.lock().unwrap().push(request.clone());
    let (status, body) = if request.starts_with("POST /token") {
        if request.contains("grant_type=refresh_token") && request.contains("refresh_token=ref1") {
            (
                200,
                r#"{"access_token":"tok2","token_type":"Bearer","expires_in":86400}"#,
            )
        } else if request.contains("grant_type=authorization_code")
            && request.contains("code=abc123")
        {
            (
                200,
                r#"{"access_token":"tok1","refresh_token":"ref1","token_type":"Bearer","expires_in":86400}"#,
            )
        } else {
            (
                400,
                r#"{"error":"invalid_request","error_description":"The code is wrong."}"#,
            )
        }
    } else if request.starts_with("GET /info") && request.contains("Bearer tok") {
        (
            200,
            r#"{"id":1,"uuid":"ffc8fdc9-5824-509e-8a57-c99b940fb996","username":"ErickSkrauch"}"#,
        )
    } else {
        (404, "{}")
    };
    let text = if status == 200 { "OK" } else { "Error" };
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 {status} {text}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
}

fn config() -> Config {
    Config {
        client_id: "my-app".into(),
        client_secret: "shh".into(),
        port: 0,
    }
}

/// The player's browser: comes back to the redirect address the page asked for, with `query`
/// (`{state}` is the page's state). A favicon request goes first, like a real browser's.
fn browser(url: &str, query: &'static str) {
    let params: std::collections::HashMap<String, String> = url
        .split_once('?')
        .unwrap()
        .1
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), decode(v)))
        .collect();
    let redirect = params["redirect_uri"].clone();
    let query = query.replace("{state}", &params["state"]);
    std::thread::spawn(move || {
        let base = redirect.trim_end_matches("/callback").to_string();
        let _ = ureq::get(&format!("{base}/favicon.ico")).call();
        let _ = ureq::get(&format!("{redirect}?{query}")).call();
    });
}

fn quiet() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn signing_in_through_the_browser_keeps_the_account_and_its_tokens() {
    let mock = Mock::start();
    let store = MemoryStore::default();
    let account = sign_in(
        &mock.endpoints(),
        &config(),
        &store,
        &|url| browser(url, "code=abc123&state={state}"),
        Duration::from_secs(10),
        &quiet(),
    )
    .unwrap();
    assert_eq!(account.name, "ErickSkrauch");
    assert_eq!(account.uuid, UUID);
    let kept: Tokens = serde_json::from_str(&store.get(&account::ely_key(UUID)).unwrap().unwrap())
        .unwrap();
    assert_eq!(kept.access_token, "tok1");
    assert_eq!(kept.refresh_token.as_deref(), Some("ref1"));
    assert!(kept.expires_at > now() + 80_000);
    // What went to Ely.by: the code with the application's id, secret and the exact redirect.
    let requests = mock.requests();
    let token = requests.iter().find(|r| r.starts_with("POST /token")).unwrap();
    assert!(token.contains("client_id=my-app") && token.contains("client_secret=shh"));
    assert!(token.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A"));
    assert!(requests.iter().any(|r| r.starts_with("GET /info")));
}

#[test]
fn a_refusal_in_the_browser_keeps_nothing() {
    let mock = Mock::start();
    let store = MemoryStore::default();
    let err = sign_in(
        &mock.endpoints(),
        &config(),
        &store,
        &|url| browser(url, "error=access_denied&error_message=Nope&state={state}"),
        Duration::from_secs(10),
        &quiet(),
    )
    .unwrap_err();
    assert_eq!(err, "Nope");
    assert_eq!(store.get(&account::ely_key(UUID)).unwrap(), None);
    assert!(!mock.requests().iter().any(|r| r.starts_with("POST /token")));
}

#[test]
fn a_way_back_from_another_sign_in_is_not_exchanged() {
    let mock = Mock::start();
    let store = MemoryStore::default();
    let err = sign_in(
        &mock.endpoints(),
        &config(),
        &store,
        &|url| browser(url, "code=abc123&state=somebody-elses"),
        Duration::from_secs(10),
        &quiet(),
    )
    .unwrap_err();
    assert!(err.contains("did not start here"), "{err}");
    assert!(!mock.requests().iter().any(|r| r.starts_with("POST /token")));
}

#[test]
fn a_wrong_code_is_ely_bys_refusal() {
    let mock = Mock::start();
    let err = sign_in(
        &mock.endpoints(),
        &config(),
        &MemoryStore::default(),
        &|url| browser(url, "code=wrong&state={state}"),
        Duration::from_secs(10),
        &quiet(),
    )
    .unwrap_err();
    assert_eq!(err, "Ely.by refused (400): The code is wrong.");
}

#[test]
fn a_browser_that_never_comes_back_times_out_and_cancel_stops_the_wait() {
    let mock = Mock::start();
    let err = sign_in(
        &mock.endpoints(),
        &config(),
        &MemoryStore::default(),
        &|_| {},
        Duration::from_millis(300),
        &quiet(),
    )
    .unwrap_err();
    assert!(err.contains("timed out"), "{err}");
    let cancelled = AtomicBool::new(true);
    let err = sign_in(
        &mock.endpoints(),
        &config(),
        &MemoryStore::default(),
        &|_| {},
        Duration::from_secs(10),
        &cancelled,
    )
    .unwrap_err();
    assert_eq!(err, "Sign-in cancelled.");
}

fn signed_in(store: &MemoryStore, expires_at: u64) -> Accounts {
    let tokens = Tokens {
        access_token: "tok1".into(),
        refresh_token: Some("ref1".into()),
        expires_at,
    };
    save_tokens(store, UUID, &tokens).unwrap();
    let mut accounts = Accounts::default();
    accounts.add_ely(UUID, "ErickSkrauch");
    accounts
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("horizon-ely-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn the_game_gets_a_session_file_for_the_ely_account_that_plays() {
    let store = MemoryStore::default();
    let accounts = signed_in(&store, now() + 80_000);
    assert_eq!(accounts.player(), Player::Ely(UUID.into()));
    assert_eq!(accounts.name_arg(), None);
    let dir = temp("session");
    // A token with time left is not renewed: no request, so no mock is needed.
    let path = session_for_game(&accounts, &store, &Endpoints::default(), None, &dir)
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .unwrap();
    assert_eq!(v["name"], "ErickSkrauch");
    assert_eq!(v["uuid"], UUID);
    assert_eq!(v["access_token"], "tok1");
    assert_eq!(v["user_type"], "ely");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "only its owner reads the session file");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_ending_token_is_renewed_before_the_game_starts() {
    let mock = Mock::start();
    let store = MemoryStore::default();
    let accounts = signed_in(&store, now() + 10);
    let dir = temp("renew");
    let path = session_for_game(&accounts, &store, &mock.endpoints(), Some(&config()), &dir)
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .unwrap();
    assert_eq!(v["access_token"], "tok2");
    // The refresh token is kept: the answer has none of its own.
    let kept = load_tokens(&store, UUID).unwrap();
    assert_eq!(kept.access_token, "tok2");
    assert_eq!(kept.refresh_token.as_deref(), Some("ref1"));
    // Without the application's id and secret the renewal can't happen: an error, not a crash.
    let stale = signed_in(&store, now() + 10);
    let err = session_for_game(&stale, &store, &mock.endpoints(), None, &dir).unwrap_err();
    assert!(err.contains("account ely-setup"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn offline_players_and_a_lost_sign_in_get_no_session() {
    let store = MemoryStore::default();
    let dir = temp("none");
    let mut accounts = Accounts::default();
    accounts.play_as(Player::Offline("Builder_1".into())).unwrap();
    assert_eq!(
        session_for_game(&accounts, &store, &Endpoints::default(), None, &dir).unwrap(),
        None
    );
    // Playing as an Ely.by account whose tokens left the keychain: told to sign in again.
    accounts.add_ely(UUID, "ErickSkrauch");
    let err = session_for_game(&accounts, &store, &Endpoints::default(), None, &dir).unwrap_err();
    assert!(err.contains("sign in again"), "{err}");
    assert!(!dir.exists(), "no session file for a failed start");
}

#[test]
fn the_secret_stays_out_of_debug_output() {
    let c = config();
    assert!(!format!("{c:?}").contains("shh"));
    let t = Tokens {
        access_token: "secret-token".into(),
        refresh_token: Some("secret-refresh".into()),
        expires_at: 1,
    };
    let shown = format!("{t:?}");
    assert!(!shown.contains("secret-token") && !shown.contains("secret-refresh"));
}
