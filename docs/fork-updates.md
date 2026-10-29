# Updates in this fork

The launcher updates only from **this fork's own repository**,
<https://github.com/amreVault/horizon-no-ms-login>. It never asks upstream Horizon
(`horizon-rust/horizon`) for a release, so upstream's launcher (the one with Microsoft sign-in) can't
be offered, downloaded or installed over this one.

What comes from the fork's repository (`backend::REPO`): the list of releases (the game's and the
launcher's own), each release's `SHA256SUMS` and `SHA256SUMS.sig`, the archives, the source native
mods are built from, and `launcher/links.json`.

Not updates, and still upstream's: the mod registry behind the Mods page
(`horizon-rust.github.io/horizon-mods`), the horizon-mixins source native mods build with, and the
community links in `launcher/links.json` (Discord, sponsor, website). Change them there if you want.

## When nothing is published, nothing updates

Until you publish a release in the fork's repository, "check for updates" finds nothing. That is the
safe state. To turn the launcher's self-update off for good, set `HORIZON_LAUNCHER_NO_SELF_UPDATE=1`;
development builds (`cargo run` without `--release`) never replace themselves.

## An update must be signed by your key

A launcher archive installs only when its SHA-256 is in the release's `SHA256SUMS`, and that list is
signed by a key built into the launcher (`crates/horizon-release-sign/release-keys.txt`). The file
still lists upstream Horizon's key, whose private half is not yours: **your releases will be refused
until you put your own key there.** Once you do, upstream's archives are refused too.

### One-time: your key

In PowerShell, in the project folder (the private key goes to a file; keep it outside the project and
never commit it):

    cargo run --release -p horizon-release-sign -- keygen --public-out key.pub > $HOME\horizon-release-secret.txt

Then replace the key line in `crates/horizon-release-sign/release-keys.txt` (the one that is not a
`#` comment) with the contents of `key.pub`, delete `key.pub`, commit, and rebuild the launcher:
the key is built in. Back up `horizon-release-secret.txt`; without it you can't sign updates, and
anyone who has it can.

### Each release

1. Build with the version you are releasing (the tag is `launcher@X.Y.Z`):

       $env:HORIZON_LAUNCHER_VERSION="0.2.1"
       cd launcher
       cargo build --release

2. Put `target\release\horizon-launcher.exe` in a zip named `horizon-launcher-windows-x64.zip` in an
   otherwise empty folder, for example `dist`.
3. Sign it in Git Bash (it comes with Git for Windows, with OpenSSL), from the project folder:

       RELEASE_SIGNING_KEY=$(cat ~/horizon-release-secret.txt) tools/ci/sign_release.sh launcher@0.2.1 launcher/dist/horizon-launcher-windows-x64.zip

   This writes `SHA256SUMS` and `SHA256SUMS.sig` beside the zip.
4. Check it the way a launcher does:

       cargo run --release -p horizon-release-sign -- verify --tag launcher@0.2.1 launcher\dist

5. On GitHub: Releases > Draft a new release, tag `launcher@0.2.1`, not a pre-release, and attach
   all three files: the zip, `SHA256SUMS`, `SHA256SUMS.sig`.

A launcher older than the tag then offers the update, checks the signature against its built-in
key, and installs it.

The automatic workflows in `.github/workflows` are upstream's: they need its runners, secrets and
release environment, so they won't publish for you. Release by hand as above, or adapt them.
