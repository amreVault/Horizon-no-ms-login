# Offline play (no Microsoft sign-in)

This fork of [Horizon](https://github.com/horizon-rust/horizon) has **no Microsoft sign-in**. The
launcher never asks for an account and never contacts Microsoft, Xbox or the Minecraft services.
The game starts offline, or as an Ely.by account if you sign in with one
([ely-by.md](ely-by.md)).

## Playing

1. Install Minecraft: Java Edition 26.3 with any launcher and start it once.
2. Open the Horizon launcher and prepare from your own install (**From my install**, or
   `horizon-launcher prepare`).
3. Pick a nickname in Settings > Account (3 to 16 letters, digits or `_`), or with
   `horizon-launcher account offline NAME`. Without a nickname the game plays as the name it made up
   (`horizon-player.txt`).
4. Press Play (`horizon-launcher play`).

Nicknames are kept in `accounts.json` in the launcher's data folder, with each one's skin and cape
look. An `accounts.json` written by the upstream launcher still loads: its Microsoft accounts are
ignored and never written back.

## What works, what doesn't

- Works: singleplayer, Open to LAN, Friends (codes and Tailscale), offline-mode servers
  (`online_mode = false`), servers you host with the launcher (always offline mode here), mods, packs.
- Doesn't: joining online-mode servers that verify a Microsoft session, and Microsoft skins and
  capes. Servers that use Ely.by accounts work after an [Ely.by sign-in](ely-by.md). An offline server never checks accounts: anyone who reaches it can join under any name, so
  keep it on a LAN or a tailnet.
- The launcher never downloads Minecraft's game files: they come from your own install.

## What this fork changed

- `launcher/src/account.rs`: offline nicknames only. Device-code sign-in, Xbox and Minecraft
  services calls, token storage and the owner gate are gone.
- The launcher's sign-in screen, the "Download and prepare" button (it needed an owning account),
  the hidden developer options, `account sign-in`, `account sign-out`, `prepare --download`,
  `server create --check-accounts` and the `account_sign_in` MCP tool are removed.
- Hosted servers always start with `--offline`.
- The game's message for online-mode servers points to the Ely.by sign-in.
- Added: Ely.by sign-in through the browser (`launcher/src/ely.rs`, `docs/ely-by.md`).
