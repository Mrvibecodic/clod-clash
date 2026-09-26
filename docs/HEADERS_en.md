# Subscription headers

What the client sends to the panel with a subscription request and what it understands in the
answer. Setting up the panel itself is in [REMNAWAVE.md](./REMNAWAVE.md) (Russian); the client is
described in the [README](./README_en.md).

Questions and help — the [Telegram chat](https://t.me/+8BJQXYXYLqM4YWYy);
news and releases — the [Telegram group](https://t.me/+2lmP1yhxpCE3MDcy).

## In short

* Standard Remnawave headers (`profile-title`, `subscription-userinfo`, `support-url` and others)
  are sent by the panel itself.
* Everything else — our `clod-*`, `announce`, `notify-*`, `fallback-*` — goes into
  **customResponseHeaders** in the subscription settings.
* Send text with Cyrillic, emoji or line breaks as `base64:<base64 text>`.
* Links must be `https://`. The exception: `support-url` and `clod-bot-url` also accept `tg:`
  and `mailto:`.
* A value the client does not understand equals a missing header.
* Headers apply only to their own subscription and never affect other subscriptions.

---

## What the client sends

These go with every subscription download: when adding, when updating, to fallback addresses
and to the new address during a move.

| Header | Value | Why |
| --- | --- | --- |
| `User-Agent` | `ClodClash/<version>`, e.g. `ClodClash/0.1.11-alpha.3` | the panel recognises the client by it and picks the answer format. If the user typed their own User-Agent in the subscription properties, that string is sent instead |
| `Accept` | `*/*` | so the panel does not return an HTML page instead of a config |
| `x-hwid` | device id, usually 32 hex characters | for the device limit |
| `x-device-os` | `Windows`, `macOS` or `Linux` | for the panel's device list |
| `x-ver-os` | OS version: `24H2`, `15.5`, `Ubuntu 24.04` | same |
| `x-device-model` | edition or model: `Windows 11 Pro`, `MacBookPro18,3 (M1 Pro)` | same |

* The four `x-*` headers are sent only while "Device identification" is on (Settings → General,
  on by default). Switched off — none of them is sent.
* The computer name is never sent. The id is derived from the system's machine id (salted and
  hashed); the machine id itself never leaves the device.
* The version may carry a suffix (`-alpha.3`), so panel rules should match the `^ClodClash`
  prefix, not an exact version.
* With "Secure connection" (a box in the subscription properties) all these values travel inside
  the encrypted request. Only a neutral browser `User-Agent` is visible in the open.

---

## `clod-*` headers

Our own headers. The "Platforms" column shows which Clod Clash clients support the header.

| Header | Value | What it does | Platforms |
| --- | --- | --- | --- |
| `clod-portal-url` | an `https://` link | the "Account" button | PC and Android |
| `clod-bot-url` | an `https://`, `tg:` or `mailto:` link | the "Bot" button | PC and Android |
| `clod-monitor-url` | an `https://` link | the "Status" button | PC and Android |
| `clod-guide-url` | an `https://` link | the "Guide" button | PC and Android |
| `clod-announce` | text | a permanent announcement, wins over `announce` | PC and Android |
| `clod-promo` | text | a promo banner with a close button | PC and Android |
| `clod-promo-url` | an `https://` link | where a click on the promo leads | PC and Android |
| `clod-lock-mode` | `true`, `lock` or `false` | locks the routing mode | PC and Android |
| `clod-ping` | `A/B` in ms, e.g. `150/300` | ping colour bounds | PC and Android |
| `clod-disable-ping` | `true` only | a tick or a cross instead of milliseconds | PC and Android |
| `clod-show-0hosts` | `true` or `false` | show the panel's placeholder nodes as they are | PC and Android |
| `clod-connect-mode` | `tun`, `proxy` or `both` | what the connect button turns on | PC only |
| `clod-simple-mode` | `true` or `false` | simple or advanced view by default | PC only |
| `clod-latency-style` | `bars`, `dot` or `number` | how the selected server's ping is drawn | PC only |
| `clod-theme` | `accent=#RRGGBB; mode=light\|dark; background=https://…` | colour, theme and window background | PC only |

Instead of `true` / `false` you can send `1` / `0`, `yes` / `no`, `on` / `off` (except
`clod-disable-ping`, where only `true` works).

No longer supported: `clod-device-remove` and `clod-hwid-limit`. On a device limit the client
shows its own dialog with the subscription name.

### Provider links

`clod-portal-url`, `clod-bot-url`, `clod-monitor-url`, `clod-guide-url` and the standard
`support-url` form a row of links on the home screen and are repeated in the settings. Only the
links you send are shown; a header disappears — so does its button.

* **Account** (`clod-portal-url`) — where the user renews the plan. It is a separate header
  because Remnawave's `profile-web-page-url` usually points to the subscription page itself.
  The client has no "Renew" or "Pay" buttons: the account page is the only place to pay.
* **Bot** (`clod-bot-url`) — the provider's bot. Separate from "Support", where a human answers.
* **Status** (`clod-monitor-url`) — the server status page.
* **Guide** (`clod-guide-url`) — "how to set it up".

### Announcement and promo

* **`clod-announce`** — the same as the standard `announce`, but if both arrive,
  `clod-announce` is shown. There is no close button: the announcement stays while the panel
  sends it. A click leads to `announce-url`.
* **`clod-promo`** — a temporary banner for offers. It can be closed. New text — the banner shows
  again. The panel stops sending it — the banner disappears on the next subscription update.
  **`clod-promo-url`** makes it clickable.
* Both are limited to 500 visible characters (300 on Android). Long text collapses to five lines
  with a "Show in full" button.
* Words can be coloured with a colour code — see [Colour in announcements](#colour-in-announcements).

### Routing mode: `clod-lock-mode`

Locks the mode (Rule / Global / Direct) to the one in the template's `mode:`; without `mode:` —
Rule. The user cannot change it in the window, from the tray or with a hotkey — a status line with
a hint is shown instead of the choice.

| Value | What happens |
| --- | --- |
| `true` | the mode is locked while the panel keeps confirming it. If the subscription has not updated successfully for over 72 hours (or three update intervals, if longer), the lock lifts itself; the next successful update brings it back |
| `lock` | the mode is locked for good: time does not lift it. It goes away only when a successful update arrives without the header or with `false` |
| `false` | no lock |

For Prizrak-Box compatibility `global-mode` is understood: `global-mode: false` is the same as
`clod-lock-mode: true`, `global-mode: true` means no lock. If both arrive, `clod-lock-mode` wins.

### Ping: `clod-ping`, `clod-disable-ping`, `clod-latency-style`

* **`clod-ping: A/B`** — green below `A` ms, yellow below `B`, red above. Two integers separated
  by `/`, `1 ≤ A < B ≤ 60000`, otherwise the header is ignored. Without it — `200/400`. Applies on
  the home screen and on the "Proxies" page.
* **`clod-disable-ping: true`** — instead of milliseconds the home screen shows a green tick
  (check passed), a red cross (failed) or a dash (not checked yet). The "Proxies" page keeps the
  numbers.
* **`clod-latency-style`** (PC only) — how the selected server's ping is drawn on the home
  screen: `bars` — four bars (the default), `dot` — a coloured dot, `number` — a number.
  Synonyms: `signal` = `bars`, `dots` = `dot`, `ms` and `latency` = `number`;
  `pxa-latency-dots: 1` = `dot`.

### Placeholder nodes: `clod-show-0hosts`

Without the header the client recognises the panel's placeholder nodes and shows a screen with
the reason instead (see [Placeholder nodes](#placeholder-nodes)). With `true` the client parses
nothing: the panel's nodes go into the server list under their own names, there are no "no
servers" screens, and such nodes have no ping. The device-limit dialog does not depend on this
header.

### Connection method: `clod-connect-mode` (PC only)

What the connect button turns on: `tun` — TUN only, `proxy` — the system proxy only, `both` —
both. Synonyms: `tunnel`, `vpn` = `tun`; `system`, `system-proxy`, `sysproxy` = `proxy`; `all` =
`both`. Without the header — the system proxy.

* If the user picked the connection method themselves, their choice wins.
* If `clod-lock-mode` came along, the header decides: the connection switches are replaced by a
  line "… — the connection method is set by your provider", and a method the header does not name
  is switched off by the app.

### Interface view: `clod-simple-mode` (PC only)

`true` — the simple view, `false` — the advanced one. Without the header — simple. The user's
choice always wins. Synonyms: `pxa-simple-mode` (Prizrak-Box), `flclashx-newboard` (FlClashX).

### Styling: `clod-theme` (PC only)

```
clod-theme: accent=#2E7CF6; mode=dark; background=https://cdn.provider.example/bg.jpg
```

| Field | Value | What it does |
| --- | --- | --- |
| `accent` | colour `#RRGGBB` (the hash is optional) | accent colour of buttons and switches |
| `mode` | `light` or `dark` | light or dark theme |
| `background` | an `https://` image, up to 4 MB | window background. Downloaded with the subscription update and stored on the computer |

* Fields are separated by `;` in any order, any of them may be omitted. Unknown fields and bad
  values are skipped, the rest are applied.
* The user's choice wins: a colour set in the settings overrides `accent`, a manually chosen
  theme overrides `mode` (the header applies only with the "follow system" theme). The "Provider
  styling" switch in the theme settings turns the header off entirely.
* Applies only to the active subscription. The header is gone — styling returns to the user's
  settings.
* It is a closed list of fields, not CSS: the provider cannot redraw the interface or inject
  code into the window.

---

## Standard headers

### Subscription description

| Header | Value | What it does |
| --- | --- | --- |
| `profile-title` | text | subscription name, refreshed with every answer. If the user set their own name, "Own (from the panel)" is shown |
| `content-disposition` | `filename=…` | fallback name when there is no `profile-title` |
| `profile-logo` | an `https://` image, up to 2 MB | logo. Downloaded with the subscription update and stored on the computer |
| `subscription-userinfo` | `upload=…; download=…; total=…; expire=…` | traffic and expiry. `total=0` — "Unlimited", `expire=0` — "No expiry"; `expire` in milliseconds is understood too |
| `subscription-refill-date` | unix time | "Traffic resets on {date}" |
| `profile-update-interval` | hours | auto-update interval, see below |
| `Date` | a regular HTTP header | the panel's clock: expiry is counted by it, see below |
| `profile-web-page-url` | an `https://` link | the "Home" item in the subscription card menu |
| `support-url` | an `https://`, `tg:` or `mailto:` link | the "Support" button |
| `announce` | text | a permanent announcement (if there is no `clod-announce`) |
| `announce-url` | an `https://` link | where a click on the announcement leads |

**Update interval.** The interval from `profile-update-interval` is applied on every subscription
update unless the user set their own. If it arrived when the subscription was added, the "Update
Interval" field in its properties is locked with an explanation. `0` turns auto-update off.
Without the header and without the user's own interval the subscription is not updated on a
schedule.

**Expiry and the panel's clock.** On every update the client compares its clock with the `Date`
header and counts expiry by the panel's clock — a wrong clock on the computer does not shift it.
A cached answer (`Age` above zero) is not used for this. A minute and a half after `expire` the
client updates the subscription once by itself — to show a renewal or the expired state right
away. This works even without an update interval; an unchecked "Allow Auto Update" forbids it.

### Subscription address: fallback and new

| Header | Value | What it does |
| --- | --- | --- |
| `fallback-url` | a full `https://` address | a fallback address of the same panel |
| `fallback-domain` | a host or `host:port` | the main address with the host swapped |
| `new-url` | a full `https://` address | moves the subscription to a new address |
| `new-domain` | a host or `host:port` | a move where only the host changes; path and parameters stay |

* **How the subscription loads.** The main address first: as set in the subscription properties →
  through the client's core → through the system proxy. If no route produced a valid subscription
  — `fallback-url`, then the main address with the host from `fallback-domain`, each the same way.
  The main address itself is not changed.
* Fallback addresses come from the previous successful answer, so there are none yet when the
  subscription is first added.
* **Moving.** The new address is saved only if a trial download from it succeeds. If both
  headers are valid, `new-url` wins. At most three moves in a row (protection against two panels
  pointing at each other); the counter resets on an update without a move. A move is checked on
  subscription updates, not when adding.
* Fallback and new addresses get the same headers as the main one (including `x-hwid`), and their
  answer is applied as the main one's.

### Device limit

| Header | Meaning | What the client does |
| --- | --- | --- |
| `x-hwid-active` | the device is registered | nothing |
| `x-hwid-not-supported` | the panel requires an id but none was sent | the "Device identification required" dialog with an "Enable" button |
| `x-hwid-max-devices-reached` or `x-hwid-limit` | the device limit is used up | the "Device limit reached" dialog with the subscription name and a "Support" button |

* Only a **200** answer with one of these headers counts as a refusal. An error answer (403, 404,
  500…) is an ordinary failed update: the previous servers stay.
* On a refusal **the previous servers are removed** — otherwise an extra device would keep using
  them. If the panel sent placeholders, they take the servers' place; if it sent an empty body or
  a page, the client itself wipes addresses and keys from the previous servers (names and rules
  stay). Other routes and fallback addresses are not tried on a refusal.
* `x-hwid-not-supported` beats `x-hwid-limit`: Remnawave sets the latter in both cases.
* While the refusal holds, the subscription card shows the reason in a red line. The servers
  come back with the first successful update after renewing or freeing a slot.

### Notifications

| Header | Value | What it does |
| --- | --- | --- |
| `notify-expire-days` | comma-separated days, e.g. `7,3,1`; `off` turns it off | expiry reminders. Without the header — `7,3,1` |
| `notify-traffic-percent` | comma-separated percentages, e.g. `80,90,100`; `off` turns it off | traffic reminders. Without the header — `80,90,100` |
| `notification-subs-expire` | `true` / `false` | Happ compatibility; changes nothing: `true` gives the same `7,3,1`, and `false` does not turn reminders off (use `notify-expire-days: off` for that) |

Days are 1 to 365, percentages 1 to 100, at most ten values. Reminders are sent only for the
active subscription. The user turns them all off with the "Subscription notifications" switch
(Settings → Advanced settings → Appearance and behaviour).

---

## Parsing rules

* **Header names are case-insensitive:** `Profile-Title` and `profile-title` are the same.
* **Storage prefixes are accepted.** Any prefix ending in `-` works: `x-amz-meta-profile-title`
  reads as `profile-title`. An unrelated `renew-url` is not taken for `new-url`.
* **`base64:`** is decoded in any of four variants (standard, url-safe, padded and unpadded).
  Failed to decode — no header.
* **Cyrillic without base64** (raw UTF-8 in the value) is read too, but a line break cannot be
  sent that way — multi-line text only via `base64:`.
* **An empty value** equals a missing header.
* **Exceptions:** `Date`, `Age` and `content-disposition` are read only by their exact name,
  without prefixes or `base64:`; `subscription-userinfo` accepts a prefix but not `base64:`.

## Colour in announcements

`announce`, `clod-announce` and `clod-promo` can colour words. A `#RRGGBB` code is written right
before the word:

```
announce: #EF4444IMPORTANT: node #F59E0BNetherlands under maintenance until 05:00
```

* The colour runs from the code to the next space.
* A code in the middle of a word switches the colour: `#EF4444one#00FF00two` — "one" in red,
  "two" in green. Several codes in a row — the last one wins.
* A code followed by a space (`#EF4444 text`), a code at the very end of the text, `#XYZ`, `#12`
  stay plain text.
* Exactly six characters after the hash are taken: in `#1234567` the colour `#123456` paints `7`.
* Codes do not count towards the length limit.
* The colour is used as sent, the same in the light and the dark theme — pick shades readable on
  both.
* The syntax is compatible with Prizrak-Box. On Android colour codes are shown as plain text.

## Placeholder nodes

For an expired subscription, used-up traffic, a disabled user and unconfigured hosts Remnawave
answers not with an error but with an ordinary config where the servers are replaced by
placeholder nodes with arbitrary names ("Subscription expired", "Contact support"…).

The client recognises a placeholder by the node's shape, not by its name. A placeholder is a node
that has:

* an empty, `0.0.0.0` or `::` address, **or**
* an all-zero `uuid`, **or**
* port 0, 1 or not a number, and no credentials at all (`uuid`, `password`, `psk`, `private-key`,
  `auth`, `auth-str`, `token`).

Nodes of type `direct`, `reject`, `reject-drop`, `pass`, `dns` are never placeholders. The
`127.0.0.1` address is not a placeholder either.

Placeholders are cut out before the config reaches the core. Instead of an empty list the user
sees the reason:

| What the answer holds | What the user sees |
| --- | --- |
| a device refusal (`x-hwid-*` in a 200 answer) | "Device limit reached" |
| `expire` in the past | "Subscription expired" |
| the whole `total` used | "Out of traffic" and the reset date from `subscription-refill-date` |
| expiry and traffic are fine | "The provider sent no servers" and the placeholder names (up to four) |

Every reason comes with "Support" (if `support-url` is present) and "Update subscription"
buttons. If placeholders are only part of the list, a line "Hidden servers that cannot connect:
N" appears under the server list.

A placeholder answer replaces the previous servers: the panel says "there are no servers right
now", and the client does not argue. After renewal the first update brings the servers back.

If a group is left without a single node, the client puts `REJECT` into it — the core rejects an
empty group, and `DIRECT` would let traffic bypass the tunnel. Groups that wait for nodes from the
network (`include-all*` or a `type: http` provider) get `empty-fallback: REJECT` unless the
template sets its own. More about the template — in [REMNAWAVE.md](./REMNAWAVE.md) (Russian).

With `clod-show-0hosts: true` all of this is off — see [above](#placeholder-nodes-clod-show-0hosts).
