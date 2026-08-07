# Subscription headers

What the client sends to the panel and what it understands in the answer. Setting the panel up
lives in [REMNAWAVE.md](./REMNAWAVE.md); the client itself is described in the
[README](./README_en.md).

---

This is the whole point of the fork. Below is everything the client sends and understands.

### What the client sends

Sent with **every** subscription request — on import, on manual refresh and on the
scheduled one.

| Header | Value | Why |
| --- | --- | --- |
| `User-Agent` | `ClodClash/<app version>` | how the panel recognises the client (the `^clodclash` rule) and sees its version in the device list. Plain `name/version`, koala-clash style |
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
| `profile-logo` | provider logo URL | downloaded on every subscription update and kept locally: the logo does not blink on a cold start, works offline and is not pulled from a third-party host on every screen. A subscription added before the cache existed fetches it once, on first show. The fetch goes through the app's own core first and only then by the ordinary route — a decoration is not worth handing a third-party host the real address. `png`, `jpeg`, `webp`, `avif`, `gif`, `svg`, `bmp` and `ico` of at most 2 MiB are stored; anything else is not cached and the logo is loaded from the provider URL as before. `https` only, redirects included: a header cannot walk the client onto `http://` or into the local network |
| `subscription-userinfo` | `upload`, `download`, `total`, `expire` | traffic and expiry on the subscription card. `total=0` → "Unlimited", `expire=0` → "No expiry". Between refreshes the app adds up proxied traffic on its own and marks the sum as approximate (`≈` plus a warning triangle); the panel's own number stays the only input for "traffic exhausted", critical states and the critical states |
| `subscription-refill-date` | unix time of the traffic reset | "Traffic resets on {date}" |
| `profile-update-interval` | refresh interval in hours | sets the interval and marks it as dictated by the provider: the "Update interval" field in the subscription properties becomes disabled and says why. The value applies only when the user has not set an interval of their own; `0` turns auto-update off |
| `Date` | the panel's clock (an ordinary HTTP header) | compared with the device clock on every subscription update; the difference is stored and applied to the countdown and to the reminders, so a device with a wrong clock does not count the remaining time wrong. Nothing to configure — every server sends it. A measurement older than a month is dropped |
| `Age` | how long the answer sat in a cache (an ordinary HTTP header) | housekeeping: anything above zero means the `Date` belongs to the cache rather than to the panel, so the clock is not read off such an answer — the whole cache lifetime would land in the offset |
| `content-disposition` | file name | fallback source for the profile name when `profile-title` is absent |

**Contacting the provider**

| Header | Meaning | What the app does |
| --- | --- | --- |
| `clod-portal-url` | customer portal link | "Customer portal" button. Our own header on purpose: Remnawave's `profile-web-page-url` usually points at the subscription page itself. `https` only |
| `profile-web-page-url` | the provider's subscription page | turns the plan name on the card into a link. `https` only |
| `support-url` | support link | "Support" button; a `t.me/…` link gets a Telegram icon. `https`, `tg:` or `mailto:` only |
| `announce` | permanent provider message | banner in the app **without a close button** — lives exactly as long as the panel keeps sending it. Supports per-word colours (see below). Use `clod-promo` for one-off campaigns and `clod-hwid-limit` for the device dialog |
| `announce-url` | where clicking the banner leads | makes the `announce` banner clickable. `https` only |
| `clod-promo` | temporary promo banner | a separate accent banner the user **can dismiss**; a changed text brings it back. Same per-word colours as `announce` |
| `clod-promo-url` | where the promo click leads | makes the `clod-promo` banner clickable. `https` only |

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
| `x-hwid-not-supported` | the panel wants an id the client did not send | dialog: "The provider requires device identification. Turn it on?". **A working profile is never overwritten** — the body is the same stub as on a device limit. Outranks `x-hwid-limit`, which Remnawave sets in both blocking branches — without that precedence the user would be told about a limit they never hit |
| `x-hwid-max-devices-reached`<br>`x-hwid-limit` | device limit is full | dialog with a "Support" button; the provider's own wording comes from `clod-hwid-limit`. **A working profile is never overwritten** — the panel's body is a stub in this case. While the state holds, the subscription card carries a red line saying why it is not updating |
| `x-hwid-max-devices` | how many devices are allowed | filled into the dialog text. Remnawave 3.x does not send it — without it the dialog simply has no number |
| `clod-hwid-limit` | **optional** provider text for both device dialogs | shown under the dialog's own text — "unlink the old device in your account", say. Plain text or `base64:`, up to 500 characters, with the same `#RRGGBB` colouring as the banners. A header of its own rather than `announce`: the home banner is for everybody, this explanation is addressed to one blocked device. Without it the dialog does fine on its own wording |

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
  unpadded, url-safe and url-safe unpadded. If decoding fails the header counts as absent —
  a literal `base64:…` must never surface in a banner.
* **Non-ASCII text is read in both forms:** as `base64:` and as raw UTF-8 straight in the
  header value. The latter is not allowed by the spec, but panels do it, so the client
  copes.
* **A header value cannot contain a newline.** A multi-line announcement has to be sent as
  `base64:` — otherwise it physically cannot arrive.
* **Links are validated, plain http is banned.** `profile-logo`, `profile-web-page-url`,
  `announce-url` and every `clod-*-url` are accepted as **`https` only** — `http:`,
  `javascript:` and `file:` are dropped. `support-url` additionally understands `tg:`
  and `mailto:`, but an ordinary link there must be `https://` as well. `new-url` may not
  downgrade `https` to `http`. **There are no "Renew" and "Top up" buttons in the client**:
  the customer portal (`clod-portal-url`) is the single place the app ever points at for
  payment.
* **Empty values are ignored**, the announcement, the promo and `clod-hwid-limit` are capped
  at 500 characters, threshold
  lists are range-checked (1–365 days, 1–100 percent) and limited to ten entries. A
  completely invalid header behaves like a missing one.

### Colours in banners

`announce`, `clod-promo` and `clod-hwid-limit` can paint single words. The colour code is glued to
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

When a group ends up with no nodes at all, the client puts `REJECT` in it: mihomo answers an
empty group with `` `use` or `proxies` missing `` and refuses to start, and `DIRECT` would
leak traffic around the tunnel. The check runs last, after dangling references are cleaned
up, and does not depend on a placeholder having been found: a panel can simply send an empty
`proxies` while the group's member names come from the template — same outcome.

The only groups left alone are the ones the core fills itself: `include-all` or
`include-all-proxies` (mihomo drops `COMPATIBLE` into those), `include-all-providers` when
at least one `proxy-providers` entry exists, and `use:` naming a provider that is actually
declared. `include-all-providers` with no providers does not save a group — it gets `REJECT`
like any other.

Instead of a silent empty list the app shows **why** there is nothing to connect to, derived
from `subscription-userinfo` (which stays truthful in these responses):

| Subscription data | What the user sees | Buttons |
| --- | --- | --- |
| `expire` in the past | "Subscription expired" | "Support" (`support-url`), "Update subscription" |
| `total` used up | "Out of traffic" plus the reset date from `subscription-refill-date` | "Support", "Update subscription" |
| both look healthy | "The provider sent no servers" plus a quote of the placeholder names | "Support", "Update subscription" |

The placeholder names are **only ever quoted** ("The panel says: …") — no logic is built on
them. The "Support" button, as everywhere else, appears only when the matching header was
sent; "Update subscription" is always there — renew in the portal, come back, press it.

The last row needs confirmation from the config side: "The provider sent no servers" is
shown only when nothing survived the filter **and** placeholders were actually there. A
template that simply ships no groups is not blamed on the provider. These cards carry no
payment buttons at all: the only payment link in the whole client is the customer portal
(`clod-portal-url`) on the home screen.

---

