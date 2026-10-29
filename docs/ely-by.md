# Signing in with Ely.by

This fork has no Microsoft sign-in. Instead the launcher can sign in with an
[Ely.by](https://ely.by) account through your browser, the way other launchers do: it opens
Ely.by's page, you sign in there, and the browser comes back to the launcher. Your password is
typed on Ely.by's site only; the launcher never sees it.

With an Ely.by account the game joins servers that use Ely.by accounts (online-mode servers that
check players against `authserver.ely.by`, for example through authlib-injector). Without one, the
game plays offline as before ([offline-play.md](offline-play.md)).

## One-time setup: your own Ely.by application

Ely.by only lets a program use its OAuth2 sign-in with an application registered on its site, and
the application's secret must not be shared, so this fork ships none. Make your own (a minute):

1. Sign in at <https://account.ely.by/dev/applications/new> and create an application.
2. Type: **Website**. Redirect URI: `http://127.0.0.1:38581/callback` (exactly this).
3. Copy the application's client id and client secret, then run:

       horizon-launcher account ely-setup --client-id YOUR_ID --client-secret YOUR_SECRET

   The launcher keeps them in `ely-app.json` in its data folder (readable by you only on Unix). A
   different port: add `--port N` and use `http://127.0.0.1:N/callback` as the Redirect URI.
   `HORIZON_ELY_CLIENT_ID`, `HORIZON_ELY_CLIENT_SECRET` and `HORIZON_ELY_PORT` override the file.

## Signing in

- Window: Settings > Account > **Sign in with Ely.by**. The browser opens; finish there. The
  window shows the link in case it did not open.
- Terminal: `horizon-launcher account ely-login`.

When the browser shows "Signed in", go back to the launcher: the account is listed and plays from
the next Play. `horizon-launcher account list` lists accounts, `account select NAME` switches
between Ely.by accounts and offline names, `account ely-logout NAME` (or **Sign out** in Settings)
forgets one.

## What happens

1. The launcher listens on `127.0.0.1:38581` for one request and opens
   `https://account.ely.by/oauth2/v1` with scopes `account_info offline_access
   minecraft_server_session` and a random `state`.
2. After you sign in, Ely.by sends the browser to `/callback?code=…&state=…`. The launcher checks the
   `state`, answers with a small page, and stops listening.
3. It exchanges the code at `https://account.ely.by/api/oauth2/v1/token` (with your application's id
   and secret), asks `https://account.ely.by/api/account/v1/info` who you are, and keeps the access
   and refresh tokens in the system keychain (`ely:<uuid>`). `accounts.json` holds only the name and
   UUID.
4. On Play, a token near its end is renewed with the refresh token, and the launcher writes a
   short-lived session file (`user_type: "ely"`, owner-readable) that the game reads once and
   deletes. If the session can't be made (no network, a lost sign-in), the game starts offline as the
   account's name and the launcher says why.
5. Joining an online-mode server, the game calls `https://authserver.ely.by/session/join`
   (`HORIZON_ELY_SESSION` overrides the base address).

## Limits

- Ely.by skins and capes are not shown yet; offline names keep their local skins.
- Servers hosted by this fork's launcher are offline-mode servers. They don't verify Ely.by
  accounts.
- Servers that verify Microsoft sessions can't be joined: an Ely.by token is not one.
- The tokens are never logged or printed. Without a system keychain the launcher says so, and the
  sign-in lasts only until it closes.
