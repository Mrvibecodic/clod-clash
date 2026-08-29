# Building

How to build Clod Clash from source. Cutting a release is covered in [RELEASING.md](./RELEASING.md).

Questions and help — the [Telegram chat](https://t.me/+8BJQXYXYLqM4YWYy);
news and releases — the [Telegram group](https://t.me/+2lmP1yhxpCE3MDcy).

---

```bash
pnpm install
pnpm prebuild          # downloads the Mihomo core and helper binaries
pnpm dev               # run in development mode
pnpm build             # build an installer
```

Requires Rust (version pinned in `rust-toolchain.toml`), Node.js 22+ and pnpm. Tauri's
system dependencies are listed in the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

Checks before committing:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm exec tsc --noEmit
```

---

