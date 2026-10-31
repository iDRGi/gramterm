# gramterm-tg

TDLib-backed primitives for Gramterm.

The crate hides `tdlib-rs` generated types behind a small API:

- `TgClient::connect(Config)` — creates the TDLib client, starts the update
  loop and returns the first authorization state;
- `next_auth()` — waits for the next state and answers `waitTdlibParameters`;
- `submit_phone` / `submit_code` / `submit_password`;
- `me()` and `chats(limit)`.

It is intentionally not wired into the TUI yet. `tools/tg-probe` is the
executable harness used to validate it against a real account.

## TDLib runtime

The crate enables `tdlib-rs`'s `download-tdlib` and `static` features. The
prebuilt Linux `libtdjson` needs `libc++`, `libc++abi` and `libunwind`:

```bash
sudo apt install libc++-dev libc++abi-dev
```

On WSL without root, the runtime can be unpacked into a user directory and
exposed with `LD_LIBRARY_PATH`; see `.research/install-runtime-libs-user.sh`.

## API key

The probe takes `TG_API_ID`, `TG_API_HASH`, `TG_PHONE`, optionally `TG_CODE`
and `TG_PASSWORD`. Never commit these.
