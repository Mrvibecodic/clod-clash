# Сборка

Как собрать Clod Clash из исходников. Про выпуск релизов — в [RELEASING.md](./RELEASING.md).

Вопросы и помощь — в [Telegram-чате](https://t.me/+8BJQXYXYLqM4YWYy),
новости и релизы — в [Telegram-группе](https://t.me/+2lmP1yhxpCE3MDcy).

---

```bash
pnpm install
pnpm prebuild          # скачивает ядро Mihomo и служебные бинарники
pnpm dev               # запуск в режиме разработки
pnpm build             # сборка установщика
```

Требуется Rust (версия закреплена в `rust-toolchain.toml`), Node.js 22+ и pnpm.
Системные зависимости Tauri — по [инструкции Tauri](https://tauri.app/start/prerequisites/).

Проверки перед коммитом:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm exec tsc --noEmit
```

---

