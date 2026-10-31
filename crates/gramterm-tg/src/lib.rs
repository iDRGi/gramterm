//! TDLib-backed primitives for Gramterm.
//!
//! This crate deliberately hides TDLib's generated types behind a small
//! surface so the TUI can eventually depend on it without knowing about
//! `tdlib-rs` details.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use tdlib_rs::enums::{self, Update};
use tdlib_rs::functions;
use tokio::sync::mpsc;

const APPLICATION_NAME: &str = "Gramterm";

#[derive(Debug, Clone)]
pub struct Config {
    pub api_id: i32,
    pub api_hash: String,
    pub data_dir: PathBuf,
}

impl Config {
    pub fn new(api_id: i32, api_hash: impl Into<String>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            api_id,
            api_hash: api_hash.into(),
            data_dir: data_dir.into(),
        }
    }

    fn prepare(&self) -> Result<()> {
        std::fs::create_dir_all(self.data_dir.join("db"))
            .with_context(|| format!("cannot create {}", self.data_dir.join("db").display()))?;
        std::fs::create_dir_all(self.data_dir.join("files"))
            .with_context(|| format!("cannot create {}", self.data_dir.join("files").display()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    WaitPhoneNumber,
    WaitCode,
    WaitPassword { hint: String },
    WaitOtherDevice { link: String },
    Ready,
    Closed,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ChatInfo {
    pub id: i64,
    pub title: String,
}

pub struct TgClient {
    client_id: i32,
    config: Config,
    updates: mpsc::UnboundedReceiver<(i32, Update)>,
}

impl TgClient {
    /// Creates the TDLib client, starts its update loop and hands back the
    /// first authorization state. The first call to `next_auth` handles
    /// `waitTdlibParameters` internally.
    pub async fn connect(config: Config) -> Result<(Self, AuthState)> {
        config.prepare()?;
        tracing::info!("tdlib: creating client");
        let client_id = tdlib_rs::create_client();
        let (tx, updates) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            loop {
                match tdlib_rs::receive() {
                    Some((update, client_id)) => {
                        if tx.send((client_id, update)).is_err() {
                            break;
                        }
                    }
                    None => {
                        // `receive` times out after a couple of seconds; keep
                        // waiting instead of treating it as end-of-stream.
                    }
                }
            }
        });

        let mut client = Self {
            client_id,
            config,
            updates,
        };

        // A new client sends nothing until it gets its first request.
        tracing::info!("tdlib: get_option");
        td(functions::get_option("version".into(), client_id).await)?;
        tracing::info!("tdlib: waiting for auth");
        let state = client.next_auth().await?;
        tracing::info!("tdlib: auth state {state:?}");
        Ok((client, state))
    }

    pub fn client_id(&self) -> i32 {
        self.client_id
    }

    /// Waits for the next authorization state. `waitTdlibParameters` is
    /// answered here, so callers never see it.
    pub async fn next_auth(&mut self) -> Result<AuthState> {
        loop {
            tracing::debug!("tdlib: waiting for update");
            let Some((client_id, update)) = self.updates.recv().await else {
                bail!("TDLib update channel closed");
            };
            tracing::debug!("tdlib: update received for client {client_id}");
            if client_id != self.client_id {
                continue;
            }
            let Update::AuthorizationState(state) = update else {
                tracing::debug!("tdlib: non-auth update");
                continue;
            };
            tracing::info!("tdlib: auth update");
            match state.authorization_state {
                enums::AuthorizationState::WaitTdlibParameters => {
                    tracing::info!("tdlib: setting parameters");
                    self.set_parameters().await?;
                    tracing::info!("tdlib: parameters set");
                }
                other => return Ok(map_auth(other)),
            }
        }
    }

    async fn set_parameters(&self) -> Result<()> {
        let db = self.config.data_dir.join("db");
        let files = self.config.data_dir.join("files");
        td(functions::set_tdlib_parameters(
            false,
            path_string(&db),
            path_string(&files),
            String::new(),
            true,
            true,
            true,
            true,
            self.config.api_id,
            self.config.api_hash.clone(),
            "en".into(),
            APPLICATION_NAME.into(),
            std::env::consts::OS.into(),
            env!("CARGO_PKG_VERSION").into(),
            self.client_id,
        )
        .await)?;
        Ok(())
    }

    pub async fn submit_phone(&self, phone: &str) -> Result<()> {
        td(
            functions::set_authentication_phone_number(phone.to_string(), None, self.client_id)
                .await,
        )?;
        Ok(())
    }

    pub async fn submit_code(&self, code: &str) -> Result<()> {
        td(functions::check_authentication_code(code.to_string(), self.client_id).await)?;
        Ok(())
    }

    pub async fn submit_password(&self, password: &str) -> Result<()> {
        td(functions::check_authentication_password(password.to_string(), self.client_id).await)?;
        Ok(())
    }

    pub async fn request_qr(&self) -> Result<()> {
        td(functions::request_qr_code_authentication(Vec::new(), self.client_id).await)?;
        Ok(())
    }

    pub async fn me(&self) -> Result<UserInfo> {
        let user = td(functions::get_me(self.client_id).await)?;
        let enums::User::User(user) = user;
        Ok(UserInfo {
            id: user.id,
            name: format!("{} {}", user.first_name, user.last_name)
                .trim()
                .to_string(),
        })
    }

    /// Loads and resolves the first `limit` chats. TDLib returns ids from
    /// `getChats`; titles require a `getChat` per id.
    pub async fn chats(&self, limit: i32) -> Result<Vec<ChatInfo>> {
        // 404 means there are no more chats; the ids already loaded are fine.
        let _ = td(functions::load_chats(None, limit, self.client_id).await);
        let chats = td(functions::get_chats(None, limit, self.client_id).await)?;
        let enums::Chats::Chats(chats) = chats;

        let mut out = Vec::with_capacity(chats.chat_ids.len());
        for chat_id in chats.chat_ids {
            let chat = td(functions::get_chat(chat_id, self.client_id).await)?;
            let enums::Chat::Chat(chat) = chat;
            out.push(ChatInfo {
                id: chat.id,
                title: chat.title,
            });
        }
        Ok(out)
    }
}

fn map_auth(state: enums::AuthorizationState) -> AuthState {
    match state {
        enums::AuthorizationState::WaitPhoneNumber => AuthState::WaitPhoneNumber,
        enums::AuthorizationState::WaitCode(_) => AuthState::WaitCode,
        enums::AuthorizationState::WaitPassword(password) => AuthState::WaitPassword {
            hint: password.password_hint,
        },
        enums::AuthorizationState::WaitOtherDeviceConfirmation(confirmation) => {
            AuthState::WaitOtherDevice {
                link: confirmation.link,
            }
        }
        enums::AuthorizationState::Ready => AuthState::Ready,
        enums::AuthorizationState::Closed => AuthState::Closed,
        other => AuthState::Other(format!("{other:?}")),
    }
}

fn td<T>(result: Result<T, tdlib_rs::types::Error>) -> Result<T> {
    result.map_err(|error| anyhow!("TDLib error {}: {}", error.code, error.message))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
