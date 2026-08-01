<h1 align="center">
  Clod Clash
</h1>

<p align="center">
  A desktop client for Remnawave subscriptions, powered by the
  <a href="https://github.com/MetaCubeX/mihomo">Mihomo</a> core.
  <br>
  A fork of <a href="https://github.com/clash-verge-rev/clash-verge-rev">Clash Verge Rev</a>.
</p>

<p align="center">
  Languages: <a href="../README.md">Русский</a> · <b>English</b>
</p>

---

## What this is

Clod Clash is Clash Verge Rev turned into a client for a panel's customers: the user
pastes a subscription link and presses one button. Everything the panel wants to tell the
client — plan name, logo, announcement, links to the customer portal and support, the
device limit, a change of subscription address — is understood and shown.

The Mihomo core is never modified: the official binary and its regular REST/IPC interface
are used. All of Clash Verge Rev's technical surface (rules, connections, logs, config
editors) is kept — it is simply moved out of sight into an advanced mode.

> **Status: alpha.** Releases ship as pre-releases; auto-update works (the `updater`
> channel). Panel setup guide: [REMNAWAVE.md](./REMNAWAVE.md) (Russian).

## Differences from Clash Verge Rev

| Capability | Clash Verge Rev | Clod Clash |
| --- | --- | --- |
| Remnawave / Happ subscription headers | 4 headers | 28 headers, see below |
| Device identity (`x-hwid`) | no | yes, including device-limit handling |
| Spare subscription address | no | `fallback-url` and `fallback-domain` |
| Provider-driven address change | no | `new-url` / `new-domain`, verified before adopting |
| Logo, announcements, portal, support | partial | yes |
| Unmetered traffic / no expiry | shows `0 B` and `-` | "Unlimited" / "No expiry" |
| Updating the Mihomo core separately from the app | no | yes (managed core in the settings) |
| Simple interface mode | no | yes — a single Connect button |
| Server selection | reset by delay tests and subscription updates | strictly preserved; starred servers float to the top and replace a dead selection |

---

## Subscription headers

This is the whole point of the fork. Below is everything the client sends and understands.

### What the client sends

Sent with **every** subscription request — on import, on manual refresh and on the
scheduled one.

| Header | Value | Why |
| --- | --- | --- |
| `User-Agent` | `ClodClash/0.0.9-alpha` | how the panel recognises the client (the `^clodclash` rule) and sees its version in the device list. Plain `name/version`, koala-clash style |
| `Accept` | `*/*` | without it the panel may take the client for a browser and serve an HTML landing page instead of the config |
| `x-hwid` | 32 hex characters | device id for the device limit |
| `x-device-os` | `Windows` / `macOS` / `Linux` | shown in the panel's device list |
| `x-ver-os` | human-readable OS version: `24H2`, `15.5`, `Ubuntu 24.04` | same |
| `x-device-model` | system edition/model: `Windows 11 Pro`, `MacBookPro18,3 (M1 Pro)`, `Ubuntu 24.04.1 LTS`. The hostname is **not** sent | same |

The four `x-*` headers are only sent while device identification is enabled (it is, by
default). Turning it off stops all of them.

### What the client understands in the response

**Subscription description**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `profile-title` | plan name | sets the profile name. A name the user typed is never overwritten |
| `profile-logo` | provider logo URL | downloaded on every subscription update and kept locally: the logo does not blink on a cold start, works offline and is not pulled from a third-party host on every screen. Only an `image/*` response of at most 2 MiB is stored. `https` only |
| `subscription-userinfo` | `upload`, `download`, `total`, `expire` | traffic and expiry on the subscription card. `total=0` → "Unlimited", `expire=0` → "No expiry" |
| `subscription-refill-date` | unix time of the traffic reset | "Traffic resets on {date}" |
| `profile-update-interval` | refresh interval in hours | sets the interval and marks it as dictated by the provider, so the user cannot override it |
| `content-disposition` | file name | fallback source for the profile name when `profile-title` is absent |

**Contacting the provider**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `clod-portal-url` | customer portal link | "Customer portal" button. Our own header on purpose: Remnawave's `profile-web-page-url` usually points at the subscription page itself. `https` only |
| `support-url` | support link | "Support" button; a `t.me/…` link gets a Telegram icon. `https`, `tg:` or `mailto:` only |
| `announce` | permanent provider message | banner in the app **without a close button** — lives exactly as long as the panel keeps sending it. Supports per-word colours (see below) |
| `announce-url` | where clicking the banner leads | makes the `announce` banner clickable. `https` only |
| `clod-promo` | temporary promo banner | a separate accent banner the user **can dismiss**; a changed text brings it back. Same per-word colours as `announce` |
| `clod-promo-url` | where the promo click leads | makes the `clod-promo` banner clickable. `https` only |
| `clod-renew-url` | plan renewal link | shows the **"Renew"** button. No header — no button. `https` only |
| `clod-topup-url` | traffic top-up link | shows the **"Top up"** button. No header — no button. `https` only |

**UI control**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `clod-simple-mode` | `1`/`0` — simple or advanced view | a hint only; the user's own choice always wins. `pxa-simple-mode` and `flclashx-newboard` are honoured too |
| `clod-lock-mode` | `1`/`0` — forbid changing modes in the app | hides the proxy/TUN toggles and the routing-mode selector, leaving a status line. `global-mode: false` (Prizrak-Box) is a synonym |

**Changing the subscription address**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `new-url` | replacement subscription URL | adopted **only** after a probe download of the candidate succeeds. The old address is kept in history |
| `new-domain` | replacement host (`host:port` allowed) | only the host changes, path and query are preserved. Verified the same way as `new-url` |
| `fallback-url` | full spare address | used only when the primary address fails. The stored address is **not** replaced |
| `fallback-domain` | spare host for the primary address | tried after `fallback-url`. Order: primary → `fallback-url` → primary with the host swapped |

At most **three** consecutive `new-url` / `new-domain` moves are followed — a guard against
two panels bouncing the client back and forth. The counter resets as soon as an update
arrives without a migration request.

**Device limit**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `x-hwid-active` | the device is registered | nothing, informational |
| `x-hwid-not-supported` | the panel wants an id the client did not send | dialog: "The provider requires device identification. Turn it on?" |
| `x-hwid-max-devices-reached`<br>`x-hwid-limit` | device limit is full | dialog with the text from `announce` and a "Support" button. **A working profile is never overwritten** — the panel's body is a stub in this case |
| `x-hwid-max-devices` | how many devices are allowed | filled into the dialog text |

**Reminders**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `notify-expire-days` | how many days ahead to warn: `7,3,1` or `off` | system notifications before the subscription expires |
| `notify-traffic-percent` | used-traffic thresholds: `80,90,100` or `off` | system notifications about traffic usage |
| `notification-subs-expire` | Happ compatibility | with none of our headers present, enables expiry reminders with the defaults |

### Parsing rules

These apply to every header above:

* **Case does not matter.** `Profile-Title`, `profile-title` and `PROFILE-TITLE` are the same.
* **Object-storage prefixes are accepted.** When a subscription is served from S3-compatible
  storage the headers arrive as `x-amz-meta-profile-title`, `x-obs-meta-support-url` and
  friends — those are recognised. An unrelated header such as `renew-url` is **not**
  mistaken for `new-url`.
* **A `base64:` prefixed value is decoded.** Four alphabets are understood: standard,
  unpadded, url-safe and url-safe unpadded. If decoding fails, the raw string is used.
* **Non-ASCII text is read in both forms:** as `base64:` and as raw UTF-8 straight in the
  header value. The latter is not allowed by the spec, but panels do it, so the client
  copes.
* **A header value cannot contain a newline.** A multi-line announcement has to be sent as
  `base64:` — otherwise it physically cannot arrive.
* **Links are validated, plain http is banned.** `profile-logo`, `profile-web-page-url`,
  `announce-url` and every `clod-*-url` are accepted as **`https` only** — `http:`,
  `javascript:` and `file:` are dropped. `support-url` additionally understands `tg:`
  and `mailto:`. `new-url` may not downgrade `https` to `http`.
* **Empty values are ignored**, an announcement is capped at 500 characters, threshold
  lists are range-checked (1–365 days, 1–100 percent) and limited to ten entries. A
  completely invalid header behaves like a missing one.

### Colours in banners

`announce` and `clod-promo` can paint single words. The colour code is glued to
the word, with no space in between:

```
announce: #EF4444IMPORTANT: the #F59E0BNetherlands node is under maintenance until 05:00
```

* one word is painted — from the code to the next space;
* the syntax is Prizrak-Box compatible, so a panel already configured for that
  client works here unchanged;
* the code does not count against the 500 character cap — only visible text does;
* `#EF4444` **followed by a space**, `#XYZ`, `#12` and a plain hash stay text;
* the colour is used exactly as sent, identically in light and dark themes — the
  app does not bend a provider's brand colour to its own palette.

### Panel placeholder nodes

For an expired subscription, exhausted traffic quota, a disabled user or unconfigured
hosts Remnawave answers with **HTTP 200 and a valid config** rather than an error — one
where the servers are replaced by placeholder nodes: `server: 0.0.0.0`, `port: 1`, a nil
`uuid`. Their names are arbitrary — "Subscription expired", "Contact support",
"→ No hosts found", or whatever the panel admin configured.

The client **drops those nodes before the config reaches the core**: they never show up in
the server list, never take part in a latency test and can never be picked automatically.
The check is structural (address, port, nil identifier), not name-based — panels localise
those names and change them at will. A loopback address (`127.0.0.1`) is **not** treated as
a placeholder: a local relay is a legitimate setup.

When a group ends up with no nodes at all, the client puts `REJECT` in it: the core rejects
an empty group, and `DIRECT` would leak traffic around the tunnel.

Instead of a silent empty list the app shows **why** there is nothing to connect to, derived
from `subscription-userinfo` (which stays truthful in these responses):

| Subscription data | What the user sees | Buttons |
| --- | --- | --- |
| `expire` in the past | "Subscription expired" | "Renew" (`clod-renew-url`), "Support" |
| `total` used up | "Out of traffic" plus the reset date from `subscription-refill-date` | "Top up" (`clod-topup-url`), "Renew", "Support" |
| both look healthy | "The provider sent no servers" plus a quote of the placeholder names | "Support", "Update subscription" |

The placeholder names are **only ever quoted** ("The panel says: …") — no logic is built on
them. Buttons, as everywhere else, appear only when the matching header was sent.

---

## Configuring a Remnawave panel

**User-Agent rule.** Remnawave's default subscription-response rules do not know about this
client. Add a rule matching `^clodclash` with the **MIHOMO** format, otherwise the panel
serves its default response and the app reports that the panel did not recognise the client.

**Extra headers.** `announce`, `announce-url`, `profile-logo`, `support-url`, `new-url`,
`fallback-url`, `notify-*`, our `clod-*` family and the rest that are not part of
Remnawave's standard set are configured through `customResponseHeaders`. Values with
non-ASCII text are safer to send as `base64:<payload>`; every link must be `https://`.

**The template does not drive the client.** `mode`, ports, `tun` and
`external-controller` from the template are overwritten by the app's own settings;
`profile.store-selected` is forced to `true`, so the chosen server survives
subscription updates. The full template guide lives in [REMNAWAVE.md](./REMNAWAVE.md).

**Device limit.** With the limit enabled the panel refuses to serve the subscription without
`x-hwid`. The client sends it by default, and the id is stable across restarts and app
updates, so a device is not registered twice.

---

## Device id

Derived from the operating system's machine id:

| System | Source |
| --- | --- |
| Windows | `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` |
| macOS | `IOPlatformUUID` |
| Linux | `/etc/machine-id`, falling back to `/var/lib/dbus/machine-id` |

The value is salted, hashed with SHA-256, and the first 32 hex characters are what leaves
the machine. **The machine id itself never does**, and the hostname is never sent at all. The result satisfies Remnawave 2.9's
`^[a-zA-Z0-9=-]{10,64}$` check and is cached in the app config so it survives a change in
how the underlying id is read. When no stable source is available a random id is generated
and cached the same way.

The id is sent **only** to the subscription address and nowhere else. The
**"Device identification"** switch in Settings → General turns it off; the tooltip next to
it shows exactly what goes to the panel — `x-hwid`, `x-device-os`, `x-ver-os`,
`x-device-model` and the `User-Agent`. With identification off none of them is sent, and a
panel that enforces a device limit may refuse the subscription — the app then offers to
turn identification back on.

---

## Building

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

## Acknowledgements

Clod Clash would not exist without these projects:

* [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo) — the core everything runs on.
  We neither modify nor fork it: the official binary is used.
* [clash-verge-rev/clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev) —
  the application Clod Clash forks. The entire interface, profile handling, system proxy,
  TUN, service and tray are their work.
* [zzzgydi/clash-verge](https://github.com/zzzgydi/clash-verge) — the original Clash Verge
  this line of clients started from.
* [tauri-apps/tauri](https://github.com/tauri-apps/tauri) — the application framework.
* [Dreamacro/clash](https://github.com/Dreamacro/clash) — the ancestor of the core.
* [remnawave/panel](https://github.com/remnawave/panel) — the panel this fork targets; the
  base set of subscription headers comes from its implementation.

Separately, to the projects whose product decisions and header sets we studied:
[FlClash](https://github.com/chen08209/FlClash) and its fork
[FlClashX](https://github.com/pluralplay/FlClashX),
[koala-clash](https://github.com/coolcoala/koala-clash),
[Prizrak-Box](https://github.com/legiz-ru/Prizrak-Box).

## License

GPL-3.0, same as Clash Verge Rev. See [LICENSE](../LICENSE).
