use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use tdlib_rs::enums::{self, Update};
use tdlib_rs::functions;
use tokio::sync::mpsc;

const APP: &str = "Gramterm";

fn td<T>(result: Result<T, tdlib_rs::types::Error>) -> Result<T> {
    result.map_err(|error| anyhow!("TDLib error {}: {}", error.code, error.message))
}

#[tokio::main]
async fn main() -> Result<()> {
    let api_id = std::env::var("TG_API_ID")
        .context("set TG_API_ID")?
        .parse::<i32>()
        .context("TG_API_ID must be a number")?;
    let api_hash = std::env::var("TG_API_HASH").context("set TG_API_HASH")?;
    let data_dir = data_dir()?;
    std::fs::create_dir_all(data_dir.join("db"))?;
    std::fs::create_dir_all(data_dir.join("files"))?;

    let client_id = tdlib_rs::create_client();
    let (tx, mut rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Some((update, client_id)) = tdlib_rs::receive() {
            if tx.send((client_id, update)).is_err() {
                break;
            }
        }
    });

    // A new client sends nothing until it gets its first request.
    td(functions::get_option("version".into(), client_id).await)?;

    let mut pending_phone = std::env::var("TG_PHONE").ok();

    while let Some((event_client, update)) = rx.recv().await {
        if event_client != client_id {
            continue;
        }
        let Update::AuthorizationState(state) = update else {
            continue;
        };

        match state.authorization_state {
            enums::AuthorizationState::WaitTdlibParameters => {
                let db = data_dir.join("db").to_string_lossy().into_owned();
                let files = data_dir.join("files").to_string_lossy().into_owned();
                td(functions::set_tdlib_parameters(
                    false,
                    db,
                    files,
                    String::new(),
                    true,
                    true,
                    true,
                    true,
                    api_id,
                    api_hash.clone(),
                    "en".into(),
                    APP.into(),
                    std::env::consts::OS.into(),
                    env!("CARGO_PKG_VERSION").into(),
                    client_id,
                )
                .await)?;
            }
            enums::AuthorizationState::WaitPhoneNumber => {
                let phone = match pending_phone.take() {
                    Some(phone) => phone,
                    None => ask("Phone (international format): ")?,
                };
                td(functions::set_authentication_phone_number(phone, None, client_id).await)?;
            }
            enums::AuthorizationState::WaitCode(_) => {
                let code = env_or_ask("TG_CODE", "Login code: ")?;
                td(functions::check_authentication_code(code, client_id).await)?;
            }
            enums::AuthorizationState::WaitPassword(password) => {
                if !password.password_hint.is_empty() {
                    println!("2FA hint: {}", password.password_hint);
                }
                let password = env_or_ask("TG_PASSWORD", "2FA password: ")?;
                td(functions::check_authentication_password(password, client_id).await)?;
            }
            enums::AuthorizationState::WaitOtherDeviceConfirmation(confirmation) => {
                println!("QR login: {}", confirmation.link);
            }
            enums::AuthorizationState::Ready => {
                let me = td(functions::get_me(client_id).await)?;
                let enums::User::User(me) = me;
                println!(
                    "Logged in as {} {} (id {})",
                    me.first_name, me.last_name, me.id
                );

                // loadChats fills the chat list with updates; getChats then
                // returns the ids we can fetch titles for.
                let _ = td(functions::load_chats(None, 20, client_id).await);
                let chats = td(functions::get_chats(None, 20, client_id).await)?;
                let enums::Chats::Chats(chats) = chats;
                println!("Chats: {}", chats.chat_ids.len());
                for chat_id in chats.chat_ids {
                    match td(functions::get_chat(chat_id, client_id).await) {
                        Ok(enums::Chat::Chat(chat)) => {
                            println!("  {:>12}  {}", chat.id, chat.title);
                        }
                        Err(error) => println!("  {:>12}  error: {error:#}", chat_id),
                    }
                }
                break;
            }
            enums::AuthorizationState::Closed => {
                println!("TDLib closed the client");
                break;
            }
            other => {
                println!("authorization state: {other:?}");
            }
        }
    }

    Ok(())
}

fn data_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("TG_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var("HOME").context("HOME is not set; use TG_DATA_DIR")?;
    Ok(PathBuf::from(home).join(".local/share/tg-probe"))
}

fn env_or_ask(name: &str, prompt: &str) -> Result<String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => ask(prompt),
    }
}

fn ask(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim().to_string();
    if value.is_empty() {
        bail!("empty input");
    }
    Ok(value)
}
