**Horizon** — No Microsoft Login

A fork of "Horizon" (https://github.com/horizon-rust/horizon) with Microsoft authentication removed.

This fork provides two authentication options:

- Offline Mode — launch the game using a nickname.
- Ely.by — sign in to your Ely.by account.

**Running**

Windows

git clone https://github.com/amreVault/Horizon-no-ms-login.git
cd Horizon-no-ms-login/launcher
cargo run --release

Linux / macOS

git clone https://github.com/amreVault/Horizon-no-ms-login.git
cd Horizon-no-ms-login/launcher
cargo run --release

**Running the built launcher**

Windows:

.\target\release\horizon-launcher.exe

Linux / macOS:

./target/release/horizon-launcher

You need "Rust" (https://www.rust-lang.org/tools/install) to build the launcher from source.

**Purpose**

This fork removes the dependency on Microsoft account authentication and provides Offline Mode and Ely.by login instead.

**Original project**

"Forked from Horizon" (https://github.com/horizon-rust/horizon).

---

**Horizon — Без входа через Microsoft**

Форк оригинального "Horizon" (https://github.com/horizon-rust/horizon) с удалённой авторизацией Microsoft.

В форке доступны два способа авторизации:

- Offline Mode — запуск игры по никнейму.
- Ely.by — вход в аккаунт Ely.by.

**Запуск**

Windows

git clone https://github.com/amreVault/Horizon-no-ms-login.git
cd Horizon-no-ms-login/launcher
cargo run --release

Linux / macOS

git clone https://github.com/amreVault/Horizon-no-ms-login.git
cd Horizon-no-ms-login/launcher
cargo run --release

**Последующий запуск**

Windows:

.\target\release\horizon-launcher.exe

Linux / macOS:

./target/release/horizon-launcher

Для сборки лаунчера из исходного кода необходим "Rust" (https://www.rust-lang.org/tools/install).

**Смысл форка**

Форк убирает зависимость от авторизации Microsoft и добавляет альтернативные способы входа: Offline Mode и Ely.by.

**Оригинальный проект**

"Оригинальный Horizon" (https://github.com/horizon-rust/horizon).