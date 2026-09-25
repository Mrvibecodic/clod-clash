# Subscription headers

What the client sends to the panel and what it understands in the answer. Setting the panel up
lives in [REMNAWAVE.md](./REMNAWAVE.md); the client itself is described in the
[README](./README_en.md).

Questions and help — the [Telegram chat](https://t.me/+8BJQXYXYLqM4YWYy);
news and releases — the [Telegram group](https://t.me/+2lmP1yhxpCE3MDcy).

---

This is the whole point of the fork. Below is everything the client sends and understands.

### What the client sends

Sent with **every** subscription request — on import, on manual refresh and on the
scheduled one.

| Header | Value | Why |
| --- | --- | --- |
| `User-Agent` | `ClodClash/<app version>`; if the subscription's "User Agent" property is filled in, that value is sent instead and the `^ClodClash` rule may not match it | how the panel recognises the client (the `^ClodClash` rule, or `^clodclash` with `caseSensitive: false`) and sees its version in the device list. Plain `name/version`, koala-clash style |
| `Accept` | `*/*` | without it the panel may take the client for a browser and serve an HTML landing page instead of the config |
| `x-hwid` | 32 hex characters | device id for the device limit |
| `x-device-os` | `Windows` / `macOS` / `Linux` | shown in the panel's device list |
| `x-ver-os` | human-readable OS version: `24H2`, `15.5`, `Ubuntu 24.04` | same |
| `x-device-model` | system edition/model: `Windows 11 Pro`, `MacBookPro18,3 (M1 Pro)`, `Ubuntu 24.04.1 LTS`. The hostname is **not** sent | same |

The four `x-*` headers are only sent while device identification is enabled (it is, by
default). Turning it off stops all of them.

With "Secure connection" on (subscription properties) these values travel inside the
encrypted request to the relay instead of as headers; only a neutral browser `User-Agent`
is visible on the wire.

### What the client understands in the response

**Subscription description**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `profile-title` | plan name | the profile name, refreshed on every answer. If the user set a name of their own, it is shown first with the panel name in brackets: "Own (panel)" |
| `profile-logo` | provider logo URL | downloaded on every subscription update and kept locally: the logo does not blink on a cold start, works offline and is not pulled from a third-party host on every screen. A subscription added before the cache existed fetches it once, on first show. The fetch goes through the app's own core first and only then by the ordinary route — a decoration is not worth handing a third-party host the real address. `png`, `jpeg`, `webp`, `avif`, `gif`, `svg`, `bmp` and `ico` of at most 2 MiB are stored; anything else is not cached and the logo is loaded from the provider URL as before. `https` only, redirects included: a header cannot walk the client onto `http://` or into the local network |
| `subscription-userinfo` | `upload`, `download`, `total`, `expire` | traffic and expiry on the subscription card. `total=0` → "Unlimited", `expire=0` → "No expiry". Between refreshes the app adds up proxied traffic on its own and marks the sum as approximate (`≈` plus a warning triangle); the panel's own number stays the only input for "Out of traffic" and the critical states. The mark appears once 10 MB have been added; with a refresh interval of an hour or less nothing is estimated. The estimate is usually low (short connections between polls, other devices on the same subscription); the only way it can run high is a direct outbound the subscription declares under its own name, which is counted as proxied |
| `subscription-refill-date` | unix time of the traffic reset | "Traffic resets on {date}" |
| `profile-update-interval` | refresh interval in hours | sets the interval and marks it as dictated by the provider: the "Update interval" field in the subscription properties becomes disabled and says why. A new value from the panel is applied on every subscription update. If the user set an interval of their own (when adding or in the properties), the panel does not change it; `0` turns auto-update off, and then a new value is picked up only by a manual update. When the panel stops sending the header the field is unlocked again; the last value stays |
| `Date` | the panel's clock (an ordinary HTTP header) | compared with the device clock on every subscription update; the difference is stored and applied to the countdown and to the reminders, so a device with a wrong clock does not count the remaining time wrong. Nothing to configure — every server sends it. A measurement older than a month is dropped; a difference of more than a year is not stored at all |
| `Age` | how long the answer sat in a cache (an ordinary HTTP header) | housekeeping: anything above zero means the `Date` belongs to the cache rather than to the panel, so the clock is not read off such an answer — the whole cache lifetime would land in the offset |
| `content-disposition` | file name | fallback source for the profile name when `profile-title` is absent |

**Contacting the provider**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `clod-portal-url` | customer portal link | "Account" button. Our own header on purpose: Remnawave's `profile-web-page-url` usually points at the subscription page itself. `https` only |
| `profile-web-page-url` | the provider's subscription page | the "Home" item in the subscription card's context menu (right-click on the Subscriptions screen). `https` only |
| `support-url` | support link | "Support" button on the home screen, on the subscription card, in the device-limit dialog and in the settings (the provider links block and the icon in the header). `https`, `tg:` or `mailto:` only |
| `clod-bot-url` | the provider's bot | "Bot" button in the provider links row. Deliberately separate from support: a bot hands out the link, renews the plan and answers on its own, while "Support" is a person. Accepts `https`, `tg:` and `mailto:` — a bot link is almost always `tg:` |
| `clod-monitor-url` | server status page | "Status" button. `https` only |
| `clod-guide-url` | the provider's manual | "Guide" button — where to send someone asking how to set things up. `https` only |
| `announce` | permanent provider message | banner in the app **without a close button** — lives exactly as long as the panel keeps sending it. Supports per-word colours (see below). A long text collapses to 5 lines behind "Show in full", like the promo. Use `clod-promo` for one-off campaigns |
| `announce-url` | where clicking the banner leads | makes the `announce` banner clickable. `https` only |
| `clod-announce` | our variant of `announce` | the same banner; when the panel sends both, `clod-announce` is shown |
| `clod-promo` | temporary promo banner | a separate accent banner the user **can dismiss**; a changed text brings it back. Same per-word colours as `announce`. A long text is collapsed to **5 lines** behind a "Show in full" button — the app window sizes itself to its content, and an advert must not stretch it over the whole screen; the full text opens in a dialog. If the panel stops sending the header, the banner disappears on the next subscription update — so keep auto-update on for the subscription (see `profile-update-interval`) |
| `clod-promo-url` | where the promo click leads | makes the `clod-promo` banner clickable. `https` only |

**UI control**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `clod-simple-mode` | `1`/`0` — simple or advanced view | a hint only; the user's own choice always wins. `pxa-simple-mode` and `flclashx-newboard` are honoured too. Without the header — the simple view |
| `clod-show-0hosts` | `1`/`0` — show the panel's placeholder nodes as they are | without the header the client recognises the placeholders and shows its own screen with the reason instead. With `1` it interprets nothing: the panel's nodes land in the server list under their own names and the "no servers" screens never appear. See "Panel placeholder nodes" below |
| `clod-lock-mode` | `1`/`0` or `lock` — forbid changing modes in the app | hides the routing-mode selector, leaving a status line with a hint. The routing mode is the template's `mode:` (without it — Rule) and cannot be changed from the window, the tray or a hotkey. `global-mode: false` (Prizrak-Box) is a synonym for `clod-lock-mode: 1`. **The `1` lock is not forever:** it holds while the panel keeps confirming it — once the subscription has not refreshed successfully for 72 hours (or three update intervals, whichever is longer) the client releases it by itself. A panel that comes back re-applies it on the next successful refresh. **`lock` (any case) is the same lock, but permanent:** it never times out and goes away only when a successful refresh arrives without it (or with `0`). |
| `clod-latency-style` | `bars`, `dot` or `number` — how latency is drawn | cosmetic: four bars (default), a coloured dot, or milliseconds as a number. Applies to the selected-server row on the home screen. All three use the same colour thresholds. Synonyms: `signal` (= `bars`), `dots` (= `dot`), `ms` and `latency` (= `number`). `pxa-latency-dots: 1` means `dot`; `0` on that synonym means "no request", not "give me bars back" |
| `clod-disable-ping` | `true` only — hide ping numbers | instead of milliseconds a server gets a green check (probe passed), a red cross (probe failed) or a dash (not probed yet). For users confused by latency numbers. Applies on the home screen; the Proxies page still shows milliseconds. Any other value, or no header, means plain numbers |
| `clod-ping` | `A/B` — ping colour thresholds in milliseconds, e.g. `150/300` | green below `A`, yellow below `B`, red from `B` on; the bars are four below `A/2`, three below `A`, two below `B`, otherwise one. Applies on the home screen and the Proxies page, with any `clod-latency-style`. The format is strict: two whole numbers joined by `/`, spaces around them are fine, `1 ≤ A < B ≤ 60000`; anything else counts as no header at all. Without the header — `200/400`. The probe timeout, "not probed yet" and `clod-disable-ping` are not affected |
| `clod-connect-mode` | `tun`, `proxy` or `both` — how traffic is captured | decides what the Connect button raises. **The user's own choice wins**, except on a locked profile (`clod-lock-mode: 1` or `lock`), where the panel has the last word: instead of the two switches the Quick actions card shows a line naming the targets, e.g. "TUN — the connection method is set by your provider" (with `both` — "System proxy + TUN — …"); in System Setting the remaining switch is marked "Set by your provider", and the switch of a target the provider did not name is gone — the app switches that target off itself. A lock without `clod-connect-mode` leaves the switches alone. `tunnel`/`vpn`, `system`/`system-proxy`/`sysproxy` and `all` are accepted as synonyms; anything else counts as no header at all. Without the header the system proxy stays the default |
| `clod-theme` | client styling: `accent=#RRGGBB; mode=dark; background=https://…` (`mode` is `light` or `dark`) | paints the accent colour, picks the light or dark theme and sets the window background for the provider. **The user's own choice wins**: a colour set in the theme settings or an explicitly chosen theme override the header, and the "Provider styling" switch in the theme settings turns it off entirely (background included). Applies to the active subscription only. Details in "Provider styling" below |

**Changing the subscription address**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `new-url` | replacement subscription URL | adopted **only** after a probe download of the candidate succeeds. The old address is kept in history. `https://…` only |
| `new-domain` | replacement host (`host:port` allowed) | only the host changes, path and query are preserved. Verified the same way as `new-url` |
| `fallback-url` | full spare address | used only when the primary address fails. The stored address is **not** replaced. `https` only |
| `fallback-domain` | spare host for the primary address | tried after `fallback-url`. Order: primary → `fallback-url` → primary with the host swapped |

The primary address is tried along a ladder of routes: as set in the subscription
properties → through the Clod Clash core → through the system proxy; on a step that goes
through a proxy a direct request runs alongside, 250 ms behind. Only when no step brought
the subscription does the client move on to the spare addresses: `fallback-url`, then the
primary address with the host from `fallback-domain`. Each spare address walks the same
ladder. Every address has a time budget: the timeout from the subscription properties × 3
steps × 2 attempts (× 2 once more with "Secure connection") plus 10 seconds, and each spare
address gets its own. When it runs out the client reports that the subscription address did
not answer in time and moves on.

At most **three** consecutive `new-url` / `new-domain` moves are followed — a guard against
two panels bouncing the client back and forth. The counter resets as soon as an update
arrives without a migration request.

**Device limit**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `x-hwid-active` | the device is registered | nothing. Remnawave sets it on refusals too, so on its own it does not mean the device was accepted |
| `x-hwid-not-supported` | the panel wants an id the client did not send | dialog "Device identification required" with the subscription name and a "Turn on" button. **The previous servers are taken away** — same as on a device limit. Outranks `x-hwid-limit`, which Remnawave sets in both blocking branches — without that precedence the user would be told about a limit they never hit |
| `x-hwid-max-devices-reached`<br>`x-hwid-limit` | device limit is full | dialog "Device limit reached" with the subscription name and a "Support" button (when the panel sent `support-url`) — when the subscription is added and on each of its updates. **The previous servers are taken away**: the panel's placeholders replace them, and if an empty body, a web page or a link list came instead of a config, the client turns the previous servers into the same kind of placeholders itself — names and rules stay, addresses and keys are wiped, node providers are dropped. If there is nothing to turn, all traffic is rejected (`MATCH,REJECT`). Routes and spare addresses are not tried on such an answer. While the state holds, the subscription card carries a red line saying why it is not updating |

Remnawave sends a device refusal as a 200 answer, and that is what the client treats as a
refusal. An answer with an error status (403, 404, 500…) is an ordinary failed update even
if it carries `x-hwid-*` headers: the previous servers stay, there is no dialog, and the
other routes and spare addresses are tried.

**Reminders**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `notify-expire-days` | how many days ahead to warn, e.g. `7,3,1`; without the header — `7,3,1`; `off` (or `false`) turns them off, the "expired" notification included | system notifications before the subscription expires |
| `notify-traffic-percent` | used-traffic thresholds, e.g. `80,90,100`; without the header — `80,90,100`; `off` (or `false`) turns them off | system notifications about traffic usage |
| `notification-subs-expire` | Happ compatibility | accepted but changes nothing: `true` enables the same `7,3,1` that apply anyway, and `false` does not turn reminders off — use `notify-expire-days: off` for that |

Reminders cover the active subscription only. The "Subscription notifications" switch
(Settings → Advanced settings → Appearance and behaviour) turns them all off, whatever the panel sent.

### Parsing rules

These apply to every header above:

* **Case does not matter.** `Profile-Title`, `profile-title` and `PROFILE-TITLE` are the same.
* **Object-storage prefixes are accepted.** When a subscription is served from S3-compatible
  storage the headers arrive as `x-amz-meta-profile-title`, `x-obs-meta-support-url` and
  friends — those are recognised. Any prefix ending in `-` works: `…-profile-title` reads
  as `profile-title`. An unrelated header such as `renew-url` is **not** mistaken for
  `new-url`.
* **A `base64:` prefixed value is decoded.** Four alphabets are understood: standard,
  unpadded, url-safe and url-safe unpadded. If decoding fails the header counts as absent —
  a literal `base64:…` must never surface in a banner.
* **Non-ASCII text is read in both forms:** as `base64:` and as raw UTF-8 straight in the
  header value. The latter is not allowed by the spec, but panels do it, so the client
  copes.
* **A header value cannot contain a newline.** A multi-line announcement has to be sent as
  `base64:` — otherwise it physically cannot arrive.
* **Links are validated, plain http is banned.** `profile-logo`, `profile-web-page-url`,
  `announce-url`, `fallback-url`, `clod-portal-url`, `clod-monitor-url`, `clod-guide-url`,
  `clod-promo-url` and `background` in `clod-theme` are accepted as **`https` only** — `http:`,
  `javascript:` and `file:` are dropped. `support-url` and `clod-bot-url` additionally
  understand `tg:` and `mailto:`, but an ordinary link there must be `https://` as well. The
  subscription URL and `new-url` must be `https`; a subscription redirect to `http` is
  refused. The "Account" button appears **only** when `clod-portal-url` was sent — the app
  never makes up payment links of its own. **There are no "Renew" and "Top up" buttons in the client**:
  the customer portal (`clod-portal-url`) is the single place the app ever points at for
  payment.
* **Empty values are ignored**, the announcement and the promo are capped
  at 500 characters, threshold
  lists are range-checked (1–365 days, 1–100 percent), sorted, de-duplicated and limited
  to ten entries; `off` and `false` in them mean "turned off". A completely invalid header
  behaves like a missing one.
* **Boolean values** are read as `true`/`1`/`yes`/`on` and `false`/`0`/`no`/`off`
  (except `clod-disable-ping`, where only `true` works). `global-mode: true` (Prizrak-Box)
  explicitly releases the mode lock, just like `clod-lock-mode: 0`; when `clod-lock-mode`
  is sent as well, it decides.

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
* exactly six characters after the hash are taken: in `#1234567` the leftover
  `7` is what gets painted `#123456`;
* separate two painted words with a space — `#EF4444one #00FF00two`. Without it
  the second code lands inside the first word and shows up as text;
* the colour is used exactly as sent, identically in light and dark themes — the
  app does not bend a provider's brand colour to its own palette.

### Provider styling

`clod-theme` is the only header about appearance. It is a **closed list of fields**,
not CSS: a provider can suggest a colour, a theme and a background, but cannot redraw
the interface, hide the connection status or inject code into the window. Fields are
separated by semicolons, in any order:

```
clod-theme: accent=#2E7CF6; mode=dark; background=https://cdn.provider.example/bg.jpg
```

| Field | Value | What it does |
| --- | --- | --- |
| `accent` | `#RRGGBB` colour (the hash is optional) | accent colour of buttons, switches and highlights. The app adjusts its brightness for the light and dark themes itself |
| `mode` | `light` or `dark` | light or dark window theme |
| `background` | image link, `https` only | window background. The image is downloaded when the subscription updates and kept locally, like the logo: it is not fetched from a third-party host on every render and works offline. Same formats as `profile-logo`, at most 4 MB |

Rules:

* **The user's choice wins.** A colour set in the theme settings overrides `accent`;
  a manually chosen light or dark theme overrides `mode` (the header only applies while
  the theme follows the system). There is no own background in the settings: `background`
  is turned off only by the "Provider styling" switch in the theme settings, which disables
  the header entirely — for people who do not want the panel's look.
* **Only the active subscription applies.** A background auto-update of another
  subscription does not repaint the window.
* Unknown fields are skipped; invalid values (`accent=red`, `mode=auto`,
  `background=http://…`) count as absent, and the remaining fields of the same header
  still apply.
* When the header disappears from the response, the styling returns to the user's
  settings on the next subscription update.

Why not CSS: an arbitrary stylesheet from a provider is third-party code inside the
user's window, and the user has no way to repair whatever it breaks. A closed list of
fields gives the provider a branded look without that risk.

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

In place of the dropped nodes the user gets a screen with the reason — "Subscription expired",
"Out of traffic" or "The provider sent no servers". The reason comes from
`subscription-userinfo` (its expiry and traffic are real even in a placeholder response), and
with "The provider sent no servers" the placeholder names (up to four) are shown on their own
line, "Placeholder nodes removed from the subscription: …", so the provider's words are never
lost. When placeholders are only part of the list, they are dropped as well, and the server
list shows "Hidden servers that cannot connect: N (names)".

**`clod-show-0hosts: 1` switches all of that off.** The provider takes the explaining on
itself: the filter does not run, the panel's nodes land in the server list under their own
names, and there are no "no servers" screens. Such nodes will never answer a latency test —
`0.0.0.0` goes nowhere. The subscription card (expiry, traffic, percentages) behaves the same
in both modes: it reads `subscription-userinfo` and never touches the placeholders.

What the header does **not** change: the device-limit dialog. That one comes from the
`x-hwid-*` headers rather than from the nodes and is shown on every refusal sent as a 200
answer: on the limit with a "Support" button (when `support-url` was sent), on an
identification refusal with a "Turn on" button.

A placeholder response is accepted the same way in all five cases, the device limit included:
it replaces the previous configuration. That is not a side effect but the point of such a
response — the panel is saying "you have no servers right now", and the client has nothing to
overrule it with. A device over the limit used to keep using the servers it had saved, which
meant the limit did not apply to it at all. Once the subscription is renewed or a slot is
freed, the very next refresh brings the servers back — the "Update subscription" button sits
right on the status screen. On a device refusal (`x-hwid-*` in a 200 answer) the previous
configuration is replaced even when placeholders are turned off in the panel and the body is
empty: the previous servers become placeholders with the same names and no addresses or keys,
node providers are dropped. An answer with an error status is an ordinary failed update, and
the previous servers stay.

When a group ends up with no nodes at all, the client puts `REJECT` in it: mihomo answers an
empty group with `` `use` or `proxies` missing `` and refuses to start, and `DIRECT` would
leak traffic around the tunnel. The check runs last, after dangling references are cleaned
up, and does not depend on a placeholder having been found: a panel can simply send an empty
`proxies` while the group's member names come from the template — same outcome.

`REJECT` is not put into `proxies` only for groups the core fills itself: `include-all` or
`include-all-proxies`, `include-all-providers` when at least one `proxy-providers` entry
exists, and `use:` naming a provider that is actually declared. Groups that wait for nodes
from the network (`include-all*`, or a `type: http` provider) get `empty-fallback: REJECT`
unless the template set its own: until the nodes arrive the group rejects traffic instead of
letting it through directly (`COMPATIBLE`). A `proxy:` of `rule-providers` and
`proxy-providers` that ends in an emptied group is removed (for `proxy-providers`, in a
waiting group too) — otherwise they would never download. `include-all-providers` with no
providers does not save a group — it gets `REJECT` like any other.

Instead of a silent empty list the app shows **why** there is nothing to connect to, derived
from `x-hwid-*` and `subscription-userinfo` (which stays truthful in these responses); the rows
are checked top to bottom:

| What the subscription answer says | What the user sees | Buttons |
| --- | --- | --- |
| `x-hwid-max-devices-reached`/`x-hwid-limit` or `x-hwid-not-supported` | "Device limit reached"; on an identification refusal the same title carries "The panel did not recognise this device and served no servers." | "Support", "Update subscription" |
| `expire` in the past | "Subscription expired" | "Support" (`support-url`), "Update subscription" |
| `total` used up | "Out of traffic" plus the reset date from `subscription-refill-date` | "Support", "Update subscription" |
| both look healthy | "The provider sent no servers" plus a quote of the placeholder names | "Support", "Update subscription" |

The placeholder names are **only ever quoted** ("Placeholder nodes removed from the subscription: …") — no logic is built on
them. The "Support" button, as everywhere else, appears only when the matching header was
sent; "Update subscription" is always there — renew in the portal, come back, press it.

The first and last rows need confirmation from the config side: "Device limit reached" and
"The provider sent no servers" are shown only when nothing survived the filter **and** placeholders were actually there. A
template that simply ships no groups is not blamed on the provider. These cards carry no
payment buttons at all: the only payment link in the whole client is the customer portal
(`clod-portal-url`) on the home screen.

---

