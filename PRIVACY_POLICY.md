## Privacy Policy

Languages: **English** · [Русский](./PRIVACY_POLICY.ru.md)

Effective: 7 October 2026. Applies to Clod Clash for Windows, macOS and Linux.

Clod Clash is an open source VPN client. It is provided free of charge and as is.

The app has no accounts of its own, shows no advertising, and contains no analytics or
crash-reporting code. The authors run no server that receives data from the app. Everything
described below covers the requests this app makes itself.

**What the app sends, and where**

*   **To the subscription address you entered.** When the app downloads or refreshes a
    subscription, it requests the URL you entered with a `User-Agent` of the form
    `ClodClash/<version>` (or the value you typed into the subscription's "User Agent" field)
    and, if the address contains a user name and password, with those credentials. If the
    "Device identification" setting is enabled (it is by default), the request also carries
    four headers: `x-hwid`, `x-device-os` (Windows, macOS or Linux), `x-ver-os` (the system
    version) and `x-device-model` (the Windows edition, the Mac model and chip, or the Linux
    distribution name). The computer name is never sent. `x-hwid` is a pseudonymous
    identifier — a truncated SHA-256 hash of the system machine ID (`MachineGuid` on Windows,
    `IOPlatformUUID` on macOS, `/etc/machine-id` on Linux) with a fixed salt, or of a locally
    generated random value if the machine ID is unreadable. The salt is a constant published in
    the source, so the value is stable and identical across providers: a provider cannot
    recover the machine ID from it in practice, but two providers who compare notes could tell
    that two subscriptions belong to the same computer. Turning the setting off removes all
    four headers. These headers exist so that a provider can enforce its own device limits;
    whether a provider stores them is the provider's decision, described in the provider's own
    policy.
    By default the request is made directly from your network, bypassing the app's own proxy;
    with TUN mode on, the operating system routes it into the tunnel and your subscription's
    rules decide where it goes. If that attempt fails, the app retries through its own core
    and then through a system proxy set by another program, if there is one. If the provider
    has supplied a spare address (`clod-new-sub`) and the main address fails, the same request,
    with the same headers, is sent to the spare address.
*   **The secure channel.** When you add a subscription, the app first asks for it over the
    secure channel — three times at most if the provider's server does not answer — and if the
    provider has no channel, it requests the address the usual way. Over the channel the request
    goes to the same host as the subscription; the device values above, the `User-Agent` and the
    query string travel encrypted inside the request (X25519 and ChaCha20-Poly1305) instead of as
    headers, and the subscription token itself is not sent. The visible `User-Agent` of such a
    request is a generic browser string. You can turn the channel off in the subscription
    properties (the app warns that this is less safe) and on again.
*   **A quality report to your provider, over the secure channel only.** For a subscription
    that uses the secure channel, after a successful scheduled refresh (not a manual one), and
    between refreshes if they are less frequent than every 6 hours (after a failed refresh,
    from the next successful one), at most once every 6 hours, the app sends the provider's
    subscription server what the core has already measured: latency results and the "16–20"
    check results per server of that subscription, traffic volume and minutes of use per
    server, the server list entries involved (name, type, address and port from your
    subscription), the kind of network (wired, Wi-Fi, mobile) and the external IP addresses the
    measurements were made from, hour by hour. A network is identified by a hash of its
    properties (connection type and the router's MAC address, or the gateway and DNS addresses
    when there is no router MAC); the MAC and addresses themselves are never stored or sent.
    The report carries a random per-install mark, the app version and the platform; like every
    secure-channel request, it also carries the device values described above, encrypted, while
    "Device identification" is on. The Wi-Fi name, location, visited sites and addresses,
    process names and the list of programs are not included. The report is sent only if the
    provider has turned on receiving reports; the app keeps unsent measurements for at most 7
    days. Turning off the secure channel for a subscription stops its reports.
*   **To Yandex, to learn the external IP address.** For the report above, the app asks
    `ipv4-internet.yandex.net` and `ipv6-internet.yandex.net` (operated by Yandex) for the
    address your connection appears from — when the network changes or the computer wakes up,
    and at most once an hour on the same network — and only while it collects a report for a
    subscription with the secure channel. The request carries a generic browser `User-Agent`
    and no identifier or device headers, but Yandex sees your IP address (with TUN mode on,
    your subscription's rules decide whether the request goes directly or through a server).
*   **To a connectivity-check address, through each server.** Measuring latency sends a tiny
    request to a test address through every server being measured. The address is the one set
    in your subscription for that group, or the default you set in the app; when neither is
    set, the app uses `http://cp.cloudflare.com/generate_204`, operated by Cloudflare. In TUN
    mode the app also checks that traffic really flows through the tunnel by requesting
    `https://cp.cloudflare.com/generate_204` directly and through the core. These requests
    carry no identifier and no device headers, but the operator of the test address sees a
    connection from your IP address or from each server's IP.
*   **The "16–20" check.** Only if your provider turns it on with the `clod-16-20-check`
    subscription header, the app downloads 64 KB from `speed.cloudflare.com` (operated by
    Cloudflare) through each server to find servers whose traffic is being cut — when the
    network changes, when the subscription or its servers change, and at most once an hour. No
    identifier or device headers are attached.
*   **DNS.** Name resolution goes wherever your configuration says; the app adds no resolvers
    of its own. In TUN mode it turns DNS on in fake-ip mode. On macOS, while TUN mode is on in
    fake-ip mode and "Override system DNS" is enabled, the app sets the system DNS server to
    `114.114.114.114`; queries addressed to it go into the tunnel, and the previous
    setting is restored when TUN mode is turned off or the app exits.
*   **To the update and routing-data endpoints.** Checking for an app update and downloading
    it (GitHub, `Mrvibecodic/clod-clash`; at start and every 24 hours while automatic checks
    are on), updating the core when you ask for it (GitHub, `MetaCubeX/mihomo` or
    `Mrvibecodic/clod-core`), fetching routing databases (GeoIP, GeoSite, ASN — from the
    address in your configuration or from GitHub, `MetaCubeX/meta-rules-dat`), and loading a
    provider logo or group icons named by your subscription send only a standard `User-Agent`.
    No device headers and no identifier are attached to these requests. The core itself also
    downloads the rule and proxy providers your configuration lists.
*   **To your WebDAV server, if you set one up.** Backups go to the server you configured:
    subscription files and their addresses, the configuration, the settings (without the
    device identifier and the WebDAV credentials) and the locally stored measurements.
*   **To an external dashboard, if you open one.** The web dashboards in the settings open in
    your browser. For yacd the app puts the secret of the local core controller into the
    address, so that site receives it; metacubexd and zashboard keep it in the part of the
    address that is not sent to the server.
*   **Through the tunnel itself.** While the VPN is on, application traffic goes to the proxy
    servers listed in your subscription. The app does not inspect, store or forward that
    traffic anywhere else; where it ends up is defined by the configuration you supplied.

Nothing else is transmitted. The app has no server of its own.

**What stays on the computer**

Subscriptions, profiles, configuration files, credentials contained in them, selected
servers, settings, logs, unsent report measurements and "16–20" results are stored in the
app's data folder (`io.clodclash.app` in the system's application data folder, or next to the
program in portable mode). The app itself uploads them only to a WebDAV server you configured.
Local automatic backups are off by default. Logs are written locally with subscription tokens,
secrets and destination hosts masked. The support report is copied to the clipboard and the log
export is saved where you choose, both with addresses and secrets masked; they leave the
computer only if you send them yourself — review them before sharing.

**System access**

TUN mode needs administrator rights, so the app installs a system service for it; the app also
changes the system proxy settings when you turn the system proxy on. It reads the machine ID
(only to compute `x-hwid`), the router's MAC address and gateway addresses (only as a hash, to
tell networks apart) and the list of running processes (only to find its own core processes).
Process names shown on the Connections page come from the core and are never sent anywhere.
The app does not read the Wi-Fi name, location, camera, microphone or contacts.

**Who is responsible for the data**

The authors of Clod Clash do not receive, store or process the personal data of users and are
therefore not the controller (operator) of it. The data that leaves your computer goes to:

*   your subscription provider — it is the controller for the subscription requests, device
    values and quality reports it receives, and its own privacy policy governs how long it
    keeps them, what it uses them for and how you can exercise your rights;
*   the third parties named above (GitHub, Cloudflare, Yandex, the operators of addresses in
    your configuration, your WebDAV server), each under its own policy.

An IP address and a device identifier linked to a subscription can be personal data under the
GDPR, Russian Federal Law No. 152-FZ "On Personal Data" and similar laws. A provider that turns
on receiving reports or the "16–20" check should describe this in its own policy (purpose,
legal basis, retention, recipients) and inform its users.

**Your choices and rights**

*   Turn off "Device identification" in the settings: no device values are sent (a provider
    with a device limit may then refuse to give the subscription).
*   Turn off the secure channel in the subscription properties: that subscription stops sending
    reports.
*   Turn off automatic update checks in the settings.
*   Uninstall the app and delete its data folder to remove everything stored locally.
*   To access, correct or delete data a provider holds about you, or to object to its
    processing, contact that provider. You may also complain to the data protection authority
    of your country.

**International transfers**

The services the app contacts may be located outside your country — for example GitHub and
Cloudflare in the United States and Yandex in Russia; your provider's servers are wherever the
provider places them.

**Children's privacy**

The app is not directed at children under 13 and collects no personal information from anyone,
including children.

**Links to other sites**

The app can open links supplied by your subscription provider — a support page, a portal or a
provider announcement. Those sites are not operated by the authors of this app and have their
own policies.

**Security**

The secure channel encrypts its contents between the app and the provider's server, and app
updates are verified against the signature published with the release before installation. Two honest caveats: a
configuration can itself point at plain `http` addresses for its rule and proxy providers, and
those are then fetched without TLS; and the WebDAV client does not verify the server's
certificate, so use WebDAV only with a server and network you trust. No method of transmission
or storage is completely secure, so absolute security cannot be guaranteed.

**Changes to this policy**

This page is updated when the behaviour of the app changes. The version in the repository
always describes the current release; the date at the top shows when it last changed.

**Contact**

Questions and reports: open an issue in the project repository.
