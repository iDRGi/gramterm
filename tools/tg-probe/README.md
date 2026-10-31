# tg-probe

Developer-only TDLib authentication probe. It logs in with a real Telegram
account, then prints the first chats. It is not part of the Gramterm binary;
it exists to validate the TDLib bridge before wiring it into the TUI.

## Requirements

- Rust toolchain;
- TDLib runtime dependencies: `libc++`, `libc++abi`, `libunwind`, OpenSSL, zlib;
- Telegram `api_id` / `api_hash` from [my.telegram.org](https://my.telegram.org).

On Ubuntu/Debian the simplest setup is:

```bash
sudo apt install build-essential pkg-config libssl-dev libc++-dev libc++abi-dev
```

## Run

```bash
export TG_API_ID=123456
export TG_API_HASH=0123456789abcdef0123456789abcdef
export TG_PHONE='+7...'
cargo run
```

The probe asks for the login code and 2FA password on stdin. It stores its
TDLib database in `~/.local/share/tg-probe` unless `TG_DATA_DIR` is set.

Optional environment variables:

- `TG_CODE` — login code, for non-interactive runs;
- `TG_PASSWORD` — 2FA password;
- `TG_DATA_DIR` — separate TDLib data directory.

## WSL note

The prebuilt TDLib is linked against libc++. In WSL without root, the runtime
libraries can be unpacked into `~/.local/libcxx` and exposed with
`LD_LIBRARY_PATH`; see `.research/install-runtime-libs-user.sh` in the parent
workspace. For a normal developer machine, install the distro packages above.
