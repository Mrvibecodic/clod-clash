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

## What Clod Clash gives you

* **26 Remnawave / Happ subscription headers** plus five compatibility synonyms — plan name, logo,
  announcement, promo banner, customer portal, support, and the text for the device-limit
  dialog. The full list is in
  [HEADERS_en.md](./HEADERS_en.md).
* **Device identity** (`x-hwid`) with a device limit the user can actually understand.
* **A spare subscription address** (`fallback-url`, `fallback-domain`) and **a provider-driven
  address change** (`new-url`, `new-domain`) — the new address is adopted only after a successful
  probe download.
* **The subscription is fetched through the tunnel** when the direct route fails: directly first,
  then through the app's own core, then through the system proxy — on import as well as on refresh.
* **Adding a subscription takes one field**: the link; everything else is folded away, and what the
  link resolved to is shown afterwards. Subscriptions can carry group labels.
* **The state of a subscription reads at a glance**: the active one is filled with the accent
  colour, an expiring one is amber, an expired one is dimmed and underlined in red, and exhausted
  traffic is its own state.
* **Unmetered traffic and no expiry** are spelled out instead of showing "0 B" and a dash.
* **The expiry is counted on the panel's clock**, not only on the device's: the difference is
  read from the ordinary `Date` header on every subscription refresh, so a machine whose
  clock is off does not get a wrong countdown or wrong reminders — and it works offline. The
  last day is counted in hours.
* **Traffic used between subscription refreshes** is counted locally and marked as an estimate; how
  often the core is polled follows the subscription's own refresh interval.
* **TUN sets itself up**: on Windows the app installer registers the helper service, so there is no
  separate elevation prompt on install or on update; the user's choice is never erased, and a failed
  start is visible on screen, not only in the log.
* **A simple interface mode** — a single Connect button; all of Clash Verge Rev's technical side
  stays in the advanced one.
* **Quick actions on the home screen**: system proxy, TUN, start with the system and start
  minimized, without opening the settings.
* **Server selection is not reset** by delay tests or subscription updates; starred servers float
  to the top and replace one that disappeared.
* **The provider's own words about each server** — the panel puts them in the subscription (a
  host's Server description) and the client shows them under the name instead of the node type.
  What to switch on in the panel is in [REMNAWAVE.md](REMNAWAVE.md).
* **The Mihomo core updates separately from the app** — a managed core in the settings.

---

## TUN mode

TUN captures the traffic of every application, including those that ignore the system
proxy. It needs a privileged helper — a background service. In Clash Verge Rev you install
that service by hand from the settings, and until you do, the TUN switch is dead.

Here the only thing the user touches is the switch itself:

* **On Windows the app installer registers the service.** It already runs elevated, so
  there is no extra prompt on install or on update: an update replaces the service binary
  together with the app and starts it again, so the two never drift apart in version. Only
  the **service** runs elevated: the app and its WebView stay unprivileged.
* **If the service is missing anyway, the app finishes the job** — but not blindly. It first
  asks the system what it knows about the service and does the smallest thing that helps:
  registered and coming up — just wait (on an autostart the service starts later than the
  app); stopped — start it; answering but outdated — repair it; absent — install it. One
  elevation prompt (UAC on Windows, an administrator password on macOS, `pkexec` on Linux).
* **The service exists for TUN, so with TUN off it is left alone.** No privileged checks, no
  installs: a switch that is off is no reason to show a system dialog.
* **A refusal is respected, and the attempt is never repeated in a loop.** If the prompt is
  dismissed, or the service stays silent after being installed, the app keeps running through
  the system proxy and never asks on its own again — until the next version. What counts is
  the fact, not the installer's word: on Windows it reports success even when the service was
  already registered. The TUN switch still works: turning it on means "install the helper and
  turn TUN on", because now the user is the one asking.
* **The user's choice is never erased.** The app used to write `enable_tun_mode: false`
  straight into the config at startup whenever the service had not answered yet — and on an
  autostart it answers later than the app. Unavailability is now scoped to the running
  session; the settings file is left alone.
* **The core does not wait in silence.** If the service comes up slower than the app, the
  core starts without it and moves over to the service as soon as it answers, without
  dropping connections. This works on all three systems.
* **Fact, not promise.** The core's output is parsed: when mihomo cannot bring the device up
  (`Start TUN listening error: … operation not permitted`), TUN honestly turns off instead of
  staying green over a dead tunnel. A core that dies on its own is restarted — up to three
  times in a row.
* **Checking the state repairs nothing.** That check can no longer install anything: when the
  service on disk is older, the app says so once and offers to repair it in the settings. Repairs
  come either from that button or from the single per-version pass described above, and only while
  TUN is on. The check used to run on every core start — and on Windows a start is retried up to
  five times — so the elevation prompt appeared a dozen times in a row.
* **The `tun` section belongs to the app.** A provider profile (or a manual merge/script) can
  neither switch TUN off nor switch it on: a snapshot is taken before manual overrides and restored
  after, and a key that was not there does not appear.
* **The system DNS comes back even after a crash.** On macOS TUN overrides the system DNS; the
  original value and the network service name are now written next to the configs, and if the
  previous run never restored it (crash, kill, power cut), it is restored on the next start.
* **It is on the screen, not only in a toast.** While the service is being installed, a line
  under the switches (under the Connect button in the simple mode) reads "Setting TUN up —
  confirm the system prompt": it is visible behind the system dialog and explains who raised
  it. If TUN failed to come up, the same line stays as "TUN did not start, traffic is going
  around the tunnel" with a "Set up" button — a toast disappears, this does not.

---

## The subscriptions screen

**The subscription is fetched through the tunnel when the domain is blocked.** Every download —
import and refresh alike — walks a ladder: directly first, then through the app's own core, then
through the system proxy. When the provider's domain is blocked, the live channel to it is the
tunnel already running on the previous nodes. A "device not recognised" answer is not retried
through a proxy: the address is reachable, the service is answering.

**Adding takes one field.** The "Add subscription" button opens a window showing only the link your
service gave you: the name, expiry, traffic and servers arrive with it. Name, group, refresh
interval, User-Agent, timeout and the switches live in a folded "Advanced" block — when editing an
existing subscription it is open from the start, because that is what people come there for. A
second step shows what the link resolved to, so it is clear the right subscription was added. Errors
stay in the window next to the field instead of flying off as a toast, and nothing typed is lost.

**State reads at a glance.** The active subscription is filled with the accent colour and labelled
"Active", an expiring one (≤ 3 days or ≥ 90% of traffic) is amber, an expired one is dimmed and
underlined in red, and traffic exhausted while the plan is still valid is its own state. The active
subscription is never dimmed: even expired, it stays readable, because that is the one in use.

**Groups.** A subscription can carry a group label (set in its properties, where a new group is also
created), and a filter row with per-group counts appears above the grid. The group is purely visual —
the core knows nothing about it. An empty group disappears by itself, and with no groups at all the
row is not shown.

---

## Subscription headers

The full list of what the client sends and what it understands in the answer is in
[HEADERS_en.md](./HEADERS_en.md), together with the parsing rules, the coloured words in
announcements and the placeholder-node filter.

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

## Support report

Settings → Advanced → **"Support report"**, and the same button appears under any error
message. The clipboard gets a ready-made text:

* app version, OS, device, how the core is running;
* the settings that affect connectivity: core and its channel, TUN, what Connect drives,
  device identification, port, log level;
* subscription state: name, traffic and expiry from `subscription-userinfo`, HWID state,
  which headers the provider sent, whether a spare address was used;
* what the sentinel filter dropped last time;
* the tail of the app log and of the core log (800 lines each, across rotated files).

**The core's per-connection lines are not in the report.** At its normal level mihomo writes
one line per connection including the destination (`[TCP] … --> example.com:443 match …`),
and at the verbose level every DNS query as well. A raw tail of that log is a browsing
history, which has no business in a support chat, so those lines are dropped entirely — the
report says how many. Everything else — core startup, config parsing, provider errors —
stays.

**Secrets are already masked.** Any address, of any scheme — not just `http`, and including
`vless://`, `ss://` and links inside a deep link — is reduced to scheme and host: path,
query and `user:pass@` go under `***`. Whatever follows `secret`, `token`, `password`,
`passwd`, `uuid`, `authorization`, `api-key`, `x-hwid`, `hwid` or `sub-url` becomes `***`
(the list is not exhaustive and keeps growing), including when key and value are glued
together as in JSON; same for what follows `Bearer`, `Basic` and `Token`. On top of that,
any "word" of 20 characters or more drawn from `A–Z a–z 0–9 - _ = . + /` is cut when it
contains a digit or mixed case, as is any hex run of 16 or more. The home directory is
replaced with `~` so the user's own name does not travel inside a path. Deliberately blunt —
better to mask too much than to hand a subscription token to a chat.

Kept on purpose: the provider's domain (the report is useless without it), versions,
connection settings and the subscription figures.

The report is only as useful as the log that went into it, so a fresh install starts at
`debug` (an already configured app keeps whatever level it had). Rotation keeps that at
1 MB per file and eight files; the level lives in Settings → General → **"Miscellaneous"**.

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
`x-device-model` and the `User-Agent`. Turning it off drops all four `x-*` at once; the
`User-Agent` is always sent — it is what tells the panel which client is asking and which
response format to pick. A panel that enforces a device limit may then refuse the
subscription — the app offers to turn identification back on.

---

## Building

Moved to its own document: [BUILDING_en.md](./BUILDING_en.md).

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
