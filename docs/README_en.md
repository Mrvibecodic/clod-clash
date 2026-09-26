<h1 align="center">
  Clod Clash
</h1>

<p align="center">
  A client for Remnawave subscriptions on Windows, macOS and Linux.
  <br>
  The core is <a href="https://github.com/MetaCubeX/mihomo">Mihomo</a>; the second built-in core is
  <a href="https://github.com/Mrvibecodic/clod-core">Clod Core</a>, our patched fork of Mihomo.
  <br>
  A fork of <a href="https://github.com/clash-verge-rev/clash-verge-rev">Clash Verge Rev</a>.
</p>

<p align="center">
  Languages: <a href="../README.md">Русский</a> · <b>English</b>
  ·
  <a href="https://mrvibecodic.github.io/clod-clash/">Docs (Russian)</a>
  ·
  <a href="https://mrvibecodic.github.io/clod-clash/download">Download</a>
</p>

<p align="center">
  <a href="https://t.me/+2lmP1yhxpCE3MDcy">
    <img alt="Telegram group — news and releases" src="https://img.shields.io/badge/Telegram-Group%20%E2%80%94%20news-2AABEE?style=for-the-badge&logo=telegram&logoColor=white">
  </a>
  &nbsp;
  <a href="https://t.me/+8BJQXYXYLqM4YWYy">
    <img alt="Telegram chat — help and discussion" src="https://img.shields.io/badge/Telegram-Chat%20%E2%80%94%20help-229ED9?style=for-the-badge&logo=telegram&logoColor=white">
  </a>
</p>

<p align="center">
  <b><a href="https://t.me/+2lmP1yhxpCE3MDcy">Group</a></b> — news and releases
  · <b><a href="https://t.me/+8BJQXYXYLqM4YWYy">Chat</a></b> — questions, help and discussion
</p>

<p align="center">
  <img src="../website/public/screenshots/og.png" alt="Clod Clash" width="800">
</p>

---

## What this is

Paste a subscription link, press one button, and the computer is connected. Whatever the panel
says about the subscription (plan, expiry, traffic, logo, announcements, links to the account
page and support) is shown on the home screen.

The technical side of Clash Verge Rev — rules, connections, logs, config editors — is still
there: it lives in the advanced mode and the advanced settings.

<p align="center">
  <img src="../website/public/screenshots/01-home-simple.png" alt="Simple mode" width="260">
  <img src="../website/public/screenshots/03-servers.png" alt="Server selection" width="260">
  <img src="../website/public/screenshots/07-home-dark.png" alt="Dark theme" width="260">
</p>

## Features

* **One button.** Simple mode by default: connect, server, traffic and expiry. The advanced mode
  is switched on in Settings → General.
* **System proxy or TUN.** TUN captures the traffic of every program. On Windows the installer
  sets up the background service it needs, on Linux the package does, on macOS the app itself on
  first use.
* **The subscription loads even when the provider's domain is blocked.** First the way the
  subscription is set up in the client's properties (directly by default), then through the
  already running tunnel, then through the system proxy. Fallback addresses and a move to a new
  subscription address are understood too.
* **Clear states.** "Subscription expired", "Out of traffic", "Device limit reached", "The
  provider sent no servers" — in words, not as an empty server list.
* **Expiry by the panel's clock.** A wrong clock on the computer does not shift it. When the
  subscription expires, it refreshes itself to show a renewal or the expired state right away
  (retrying with pauses if that fails).
* **The chosen server sticks** through latency tests, subscription updates and restarts.
  Favourite servers sit at the top of the list.
* **A failed update does not break the working profile.** A config the core rejects is not
  applied; the previous one stays. The exception is a device-limit refusal from the panel: then
  the previous servers are removed, otherwise the limit would not work.
* **Device limit.** The device id is stable across updates; the machine id itself and the
  computer name never leave the device. Sending it can be switched off.
* **Support report** with one button — without subscription addresses, tokens or browsing history.
* **Two cores.** Stock Mihomo from MetaCubeX (the default) and
  [Clod Core](https://github.com/Mrvibecodic/clod-core), our fork of stable Mihomo: the new
  utls v1.9.0-mod-meta TLS fingerprint library (Firefox 148, Safari 26.3); a node whose check
  stalled or broke off is checked again and marked dead only if it fails that too, and an unstable
  node loses to a stable one. Switched in Settings → Advanced settings → Clash Setting → "Clash
  Core"; each core updates from its own releases.
* **Updates itself.** The "Pre-release builds" switch in Settings → General adds alpha and beta
  versions; without it only stable releases arrive.
* **Does not decide for the user.** The user's own settings beat the panel's hints; someone
  else's system proxy is left alone; an empty server group rejects traffic instead of letting it
  bypass the tunnel.

## Installation

Every build is on the [download page](https://mrvibecodic.github.io/clod-clash/download).

| System | What to download |
| --- | --- |
| Windows x64 | the `.exe` installer — installs the app, the TUN service and a firewall rule for the cores. There is also a portable `.zip`: it runs without installing but does not download updates in the background |
| macOS 11+ (Apple Silicon and Intel) | `.dmg`. The build has no Apple certificate: allow the first launch in "System Settings → Privacy & Security". Or run one command in Terminal: `curl -fsSL https://mrvibecodic.github.io/clod-clash/install-macos.sh \| bash` |
| Linux x86_64 | `.deb` or `.rpm`. On systemd systems the package registers the TUN service right away |

Details are in the ["Installation"](https://mrvibecodic.github.io/clod-clash/docs/install) section
(Russian).

## For providers

**Required:** in the panel's Subscription response rules add a rule for `User-Agent` matching
`^ClodClash` with the **MIHOMO** response format. Without it the panel sends its default answer
and the client will not accept the subscription. Everything about the panel and the template is
in [REMNAWAVE.md](./REMNAWAVE.md) (Russian).

**`clod-*` headers** are our own response headers. The panel does not send them by itself: they
go into `customResponseHeaders`. Each one applies only to its own subscription.

| Header | Value | What it does | Platforms |
| --- | --- | --- | --- |
| `clod-portal-url` | an `https://` link | the "Account" button — the page where the plan is renewed | PC and Android |
| `clod-bot-url` | an `https://`, `tg:` or `mailto:` link | the "Bot" button | PC and Android |
| `clod-monitor-url` | an `https://` link | the "Status" button — server status page | PC and Android |
| `clod-guide-url` | an `https://` link | the "Guide" button | PC and Android |
| `clod-announce` | text | a permanent announcement; if `announce` came too, `clod-announce` is shown | PC and Android |
| `clod-promo` | text | a promo banner the user can close | PC and Android |
| `clod-promo-url` | an `https://` link | where a click on the promo leads | PC and Android |
| `clod-lock-mode` | `true` / `lock` / `false` | locks the routing mode to `mode` from the template: `true` — while the panel keeps confirming it, `lock` — for good, `false` — no lock | PC and Android |
| `clod-ping` | `A/B` in ms, e.g. `150/300` | ping colour bounds: green below `A`, yellow below `B`, red above. Without the header — `200/400` | PC and Android |
| `clod-disable-ping` | `true` only | a tick or a cross instead of milliseconds | PC and Android |
| `clod-show-0hosts` | `true` / `false` | show the panel's placeholder nodes as they are instead of the client's reason screen | PC and Android |
| `clod-connect-mode` | `tun` / `proxy` / `both` | what the connect button turns on. Without the header — the system proxy | PC only |
| `clod-simple-mode` | `true` / `false` | simple or advanced view by default. Without the header — simple | PC only |
| `clod-latency-style` | `bars` / `dot` / `number` | how the selected server's ping is drawn: bars, a dot or a number. Without the header — bars | PC only |
| `clod-theme` | `accent=#RRGGBB; mode=light\|dark; background=https://…` | accent colour, light or dark theme and window background | PC only |

Instead of `true` / `false` you can send `1` / `0`, `yes` / `no`, `on` / `off` (except
`clod-disable-ping`). Send non-ASCII text as `base64:<base64 text>`. A value the client does not
understand counts as a missing header.
Every header in detail, plus the full list of standard ones, is in [HEADERS_en.md](./HEADERS_en.md).

## Documentation

* [Documentation site](https://mrvibecodic.github.io/clod-clash/) (Russian) — installation, first
  connection, settings, troubleshooting.
* [HEADERS_en.md](./HEADERS_en.md) — every subscription header: what the client sends and what it
  understands.
* [REMNAWAVE.md](./REMNAWAVE.md) (Russian) — setting up the panel and the MIHOMO template.
* [BUILDING_en.md](./BUILDING_en.md) — building from source.
* [RELEASING.md](./RELEASING.md) (Russian) — cutting a release.

## Building

In short: `pnpm install`, `pnpm prebuild`, `pnpm dev`. Details are in
[BUILDING_en.md](./BUILDING_en.md).

## Acknowledgements

Clod Clash would not exist without these projects:

* [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo) — the core everything runs on.
  By default the official binary is used unchanged; the second built-in core,
  [Clod Core](https://github.com/Mrvibecodic/clod-core), is our patched fork of Mihomo.
* [clash-verge-rev/clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev) —
  the application Clod Clash forks. The entire interface, profile handling, system proxy,
  TUN, service and tray are their work.
* [zzzgydi/clash-verge](https://github.com/zzzgydi/clash-verge) — the original Clash Verge
  this line of clients started from.
* [tauri-apps/tauri](https://github.com/tauri-apps/tauri) — the application framework.
* [Dreamacro/clash](https://github.com/Dreamacro/clash) — the ancestor of the core.
* [remnawave/panel](https://github.com/remnawave/panel) — the panel this fork targets; the
  base set of subscription headers comes from its implementation.

Components that ship with the application:

* [clash-verge-rev/clash-verge-service-ipc](https://github.com/clash-verge-rev/clash-verge-service-ipc)
  — the system service TUN cannot work without, and the protocol used to talk to it.
* [clash-verge-rev/sysproxy-rs](https://github.com/clash-verge-rev/sysproxy-rs),
  [tauri-plugin-mihomo](https://github.com/clash-verge-rev/tauri-plugin-mihomo),
  [clash-verge-logger](https://github.com/clash-verge-rev/clash-verge-logger) — the system proxy,
  the client for the core's API and the logger.
* [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat) — the `country.mmdb`,
  `geosite.dat` and `geoip.dat` databases bundled with the app.
* [Kuingsmile/uwp-tool](https://github.com/Kuingsmile/uwp-tool) — `enableLoopback.exe` for UWP
  applications on Windows; the NSIS Simple Service Plugin in the installer.
* The interface is built on [React](https://react.dev), [MUI](https://mui.com),
  [Emotion](https://emotion.sh), [i18next](https://www.i18next.com),
  [Monaco Editor](https://microsoft.github.io/monaco-editor/), SWR, ahooks and dnd-kit.

Separately, to the projects whose product decisions and header sets we studied. We took no code
from them, only formats and approaches: [FlClash](https://github.com/chen08209/FlClash) and its
fork [FlClashX](https://github.com/pluralplay/FlClashX) — the interface-mode header synonym and
the "wanted versus actual" model for TUN; [koala-clash](https://github.com/coolcoala/koala-clash) —
the User-Agent format and reporting the OS edition instead of the machine name;
[Prizrak-Box](https://github.com/legiz-ru/Prizrak-Box) — the `#RRGGBB` colour markup in
announcements, the `global-mode` synonym and the server description from the subscription;
[dropweb](https://github.com/enkinvsh/dropweb) — the logo cache guards, redacting addresses in
logs and the support report.

The country flags in the server selector are the
[HatScripts/circle-flags](https://github.com/HatScripts/circle-flags) set (MIT). They ship with the
application locally (`src/public/flags`, the license text sits next to them): emoji flags do not
render on Windows, and a client that runs on a hostile network must not fetch them over the wire.

## License

GPL-3.0, same as Clash Verge Rev. See [LICENSE](../LICENSE).

Clod Clash is a modified version of
[Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) `v2.5.2`. The modifications
were made in 2026; in the sources they carry a `clod:` marker, so what differs from upstream is
visible at a glance.

The country flags in `src/public/flags` are the
[HatScripts/circle-flags](https://github.com/HatScripts/circle-flags) set under the MIT license,
whose text sits next to them in [`src/public/flags/LICENSE`](../src/public/flags/LICENSE). The font
`src/assets/fonts/Twemoji.Mozilla.ttf` is Mozilla's build of
[Twemoji](https://github.com/jdecked/twemoji); Twemoji graphics are distributed under
[CC-BY 4.0](https://creativecommons.org/licenses/by/4.0/), copyright Twitter, Inc. and other
contributors.
