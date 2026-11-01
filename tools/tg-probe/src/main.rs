use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use gramterm_tg::{AuthState, Config, TgClient};

#[tokio::main]
async fn main() -> Result<()> {
    let api_id = std::env::var("TG_API_ID")
        .context("set TG_API_ID")?
        .parse::<i32>()
        .context("TG_API_ID must be a number")?;
    let api_hash = std::env::var("TG_API_HASH").context("set TG_API_HASH")?;

    let (mut client, mut state) =
        TgClient::connect(Config::new(api_id, api_hash, data_dir()?)).await?;
    let mut pending_phone = std::env::var("TG_PHONE").ok();

    loop {
        match state {
            AuthState::WaitPhoneNumber => {
                let phone = match pending_phone.take() {
                    Some(phone) => phone,
                    None => ask("Phone (international format): ")?,
                };
                client.submit_phone(&phone).await?;
            }
            AuthState::WaitCode => {
                let code = env_or_ask("TG_CODE", "Login code: ")?;
                client.submit_code(&code).await?;
            }
            AuthState::WaitPassword { hint } => {
                if !hint.is_empty() {
                    println!("2FA hint: {hint}");
                }
                let password = env_or_ask("TG_PASSWORD", "2FA password: ")?;
                client.submit_password(&password).await?;
            }
            AuthState::WaitOtherDevice { link } => {
                println!("QR login: {link}");
            }
            AuthState::Ready => {
                let me = client.me().await?;
                println!("Logged in as {} (id {})", me.name, me.id);
                for chat in client.api().chats(20).await? {
                    println!("  {:>12}  {}", chat.id, chat.title);
                }
                break;
            }
            AuthState::Closed => {
                println!("TDLib closed the client");
                break;
            }
            AuthState::Other(state) => {
                println!("authorization state: {state}");
            }
        }

        state = client.next_auth().await?;
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
