# Building

How to build Clod Clash from source. Cutting a release is covered in [RELEASING.md](./RELEASING.md)
(Russian).

Questions and help — the [Telegram chat](https://t.me/+8BJQXYXYLqM4YWYy);
news and releases — the [Telegram group](https://t.me/+2lmP1yhxpCE3MDcy).

---

You need Rust (the version is pinned in `rust-toolchain.toml`), Node.js 24 and pnpm 11 (as in
CI), and Tauri's system dependencies — see the [Tauri guide](https://tauri.app/start/prerequisites/).

```bash
pnpm install
pnpm prebuild          # downloads both cores, the background service and the geo databases
pnpm dev               # run in development mode
pnpm build             # build the installer
pnpm portable <target> # portable zip for Windows (after pnpm build)
```

`pnpm prebuild` puts two cores into `src-tauri/sidecar`:

* `verge-mihomo` — the latest stable Mihomo from MetaCubeX;
* `verge-mihomo-alpha` — Clod Core from the latest release of
  [Mrvibecodic/clod-core](https://github.com/Mrvibecodic/clod-core).

## Checks before committing

The same ones CI runs.

Frontend:

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm knip:check
node scripts/cleanup-unused-i18n.mjs --check
pnpm cmd:check
```

After editing `src/locales/**` regenerate the types: `pnpm i18n:types`.

Rust (from the `src-tauri` folder):

```bash
cargo clippy-all                      # on Linux; on Windows and macOS — cargo clippy-only
cargo test --lib --features clippy
cargo test --test subscription_headers --features clippy
cargo fmt --check
```

The `clippy` feature lets the Rust side build without a ready frontend (`dist`).
