# Сборка

Как собрать Clod Clash из исходников. Про выпуск релизов — в [RELEASING.md](./RELEASING.md).

Вопросы и помощь — в [Telegram-чате](https://t.me/+8BJQXYXYLqM4YWYy),
новости и релизы — в [Telegram-группе](https://t.me/+2lmP1yhxpCE3MDcy).

---

Нужны Rust (версия закреплена в `rust-toolchain.toml`), Node.js 24 и pnpm 11 (как в CI) и
системные зависимости Tauri — по [инструкции Tauri](https://tauri.app/start/prerequisites/).

```bash
pnpm install
pnpm prebuild          # скачивает оба ядра, фоновую службу и geo-базы
pnpm dev               # запуск в режиме разработки
pnpm build             # сборка установщика
pnpm portable <target> # портативный zip для Windows (после pnpm build)
```

`pnpm prebuild` кладёт в `src-tauri/sidecar` два ядра:

* `verge-mihomo` — последний стабильный Mihomo от MetaCubeX;
* `verge-mihomo-alpha` — Clod Core из последнего релиза
  [Mrvibecodic/clod-core](https://github.com/Mrvibecodic/clod-core).

## Проверки перед коммитом

То же, что гоняет CI.

Фронтенд:

```bash
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm knip:check
node scripts/cleanup-unused-i18n.mjs --check
pnpm cmd:check
```

После правки `src/locales/**` перегенерируйте типы: `pnpm i18n:types`.

Rust (из папки `src-tauri`):

```bash
cargo clippy-all                      # на Linux; на Windows и macOS — cargo clippy-only
cargo test --lib --features clippy
cargo test --test subscription_headers --features clippy
cargo fmt --check
```

Фича `clippy` нужна, чтобы Rust-часть собиралась без готового фронтенда (`dist`).
