use super::*;

#[test]
fn offline_names_are_3_to_16_letters_digits_or_underscores() {
    assert!(valid_offline_name("Builder_1"));
    assert!(valid_offline_name("abc"));
    assert!(valid_offline_name("sixteen_chars_ok_"[..16].as_ref()));
    assert!(!valid_offline_name("ab"));
    assert!(!valid_offline_name("a_name_that_is_too_long"));
    assert!(!valid_offline_name("with space"));
    assert!(!valid_offline_name("dash-ed"));
}

#[test]
fn a_nickname_plays_without_any_account() {
    let mut accounts = Accounts::default();
    assert_eq!(accounts.player(), Player::GameName);
    assert_eq!(accounts.name_arg(), None);
    accounts.play_as(Player::Offline("Builder_1".into())).unwrap();
    assert_eq!(accounts.offline, ["Builder_1"]);
    assert_eq!(accounts.player(), Player::Offline("Builder_1".into()));
    assert_eq!(accounts.name_arg(), Some("Builder_1"));
    // The same name in another case is the one already kept.
    accounts.play_as(Player::Offline("builder_1".into())).unwrap();
    assert_eq!(accounts.offline, ["Builder_1"]);
    // Back to the name the game made up; the kept names stay.
    accounts.play_as(Player::GameName).unwrap();
    assert_eq!(accounts.player(), Player::GameName);
    assert_eq!(accounts.offline, ["Builder_1"]);
}

#[test]
fn a_bad_nickname_is_refused() {
    let mut accounts = Accounts::default();
    assert!(accounts.play_as(Player::Offline("no".into())).is_err());
    assert!(accounts.offline.is_empty());
    assert_eq!(accounts.player(), Player::GameName);
}

#[test]
fn forgetting_the_playing_nickname_goes_back_to_the_games_own() {
    let mut accounts = Accounts::default();
    accounts.play_as(Player::Offline("Builder_1".into())).unwrap();
    let dir = std::env::temp_dir().join(format!("horizon-forget-{}", std::process::id()));
    accounts.remove_offline("builder_1", &dir).unwrap();
    assert!(accounts.offline.is_empty());
    assert_eq!(accounts.player(), Player::GameName);
    assert!(accounts.remove_offline("builder_1", &dir).is_err());
}

#[test]
fn an_old_accounts_file_with_microsoft_accounts_still_loads() {
    // Files written by the upstream launcher keep their offline names; the account list and the
    // selected account are ignored.
    let old = r#"{"accounts":[{"uuid":"069a79f444e94726a5befca90e38aaf5","name":"Notch"}],
        "selected":"069a79f444e94726a5befca90e38aaf5","offline":["Builder_1"],
        "offline_selected":"Builder_1"}"#;
    let accounts: Accounts = serde_json::from_str(old).unwrap();
    assert_eq!(accounts.offline, ["Builder_1"]);
    assert_eq!(accounts.name_arg(), Some("Builder_1"));
    let saved = serde_json::to_string(&accounts).unwrap();
    assert!(!saved.contains("Notch") && !saved.contains("069a79"));
}

#[test]
fn the_memory_store_keeps_and_forgets_secrets() {
    let store = MemoryStore::default();
    assert_eq!(store.get("k").unwrap(), None);
    store.set("k", "v").unwrap();
    assert_eq!(store.get("k").unwrap().as_deref(), Some("v"));
    store.delete("k").unwrap();
    assert_eq!(store.get("k").unwrap(), None);
    assert!(!store.persistent());
}

const ELY: &str = "ffc8fdc9582450968a57c99b940fb996";

#[test]
fn an_ely_account_plays_until_an_offline_name_is_picked() {
    let mut accounts = Accounts::default();
    accounts.add_ely("ffc8fdc9-5824-5096-8a57-c99b940fb996", "ErickSkrauch");
    // The UUID is kept undashed; a signed-in account plays at once.
    assert_eq!(accounts.ely[0].uuid, ELY);
    assert_eq!(accounts.player(), Player::Ely(ELY.into()));
    // The session carries its name: no `--name`.
    assert_eq!(accounts.name_arg(), None);
    assert_eq!(accounts.find_ely("erickskrauch").unwrap().uuid, ELY);
    assert_eq!(accounts.find_ely(ELY).unwrap().name, "ErickSkrauch");
    // Signing in again keeps one account, with its current name.
    accounts.add_ely(ELY, "Erick");
    assert_eq!(accounts.ely.len(), 1);
    assert_eq!(accounts.ely[0].name, "Erick");
    // An offline name takes over; the account stays signed in.
    accounts.play_as(Player::Offline("Builder_1".into())).unwrap();
    assert_eq!(accounts.player(), Player::Offline("Builder_1".into()));
    assert_eq!(accounts.name_arg(), Some("Builder_1"));
    assert_eq!(accounts.ely.len(), 1);
    accounts.play_as(Player::Ely(ELY.into())).unwrap();
    assert_eq!(accounts.player(), Player::Ely(ELY.into()));
    accounts.play_as(Player::GameName).unwrap();
    assert_eq!(accounts.player(), Player::GameName);
    assert!(accounts.play_as(Player::Ely("0".repeat(32))).is_err());
}

#[test]
fn signing_out_of_ely_by_forgets_its_tokens() {
    let store = MemoryStore::default();
    store.set(&ely_key(ELY), "{}").unwrap();
    let mut accounts = Accounts::default();
    accounts.add_ely(ELY, "Erick");
    accounts.remove_ely(ELY, &store).unwrap();
    assert!(accounts.ely.is_empty());
    assert_eq!(accounts.player(), Player::GameName);
    assert_eq!(store.get(&ely_key(ELY)).unwrap(), None);
    assert!(accounts.remove_ely(ELY, &store).is_err());
}

#[test]
fn accounts_json_holds_no_tokens() {
    let mut accounts = Accounts::default();
    accounts.add_ely(ELY, "Erick");
    let text = serde_json::to_string(&accounts).unwrap();
    assert!(text.contains(ELY) && text.contains("Erick"));
    assert!(!text.to_lowercase().contains("token"));
    let back: Accounts = serde_json::from_str(&text).unwrap();
    assert_eq!(back, accounts);
}
