# Update Log

The bilingual changelog. Each `## v{version}` section (ended by `---`)
contains an English part and a Russian part separated by `<!-- lang:en -->`
and `<!-- lang:ru -->` markers. The release workflow injects the whole
section into `latest.json` (updater notes) and into the GitHub release
body; the app's update dialog picks the part matching the UI language
(Russian UI → ru, anything else → en). Sections without markers are shown
as-is.

## v0.0.9-alpha

<!-- lang:en -->

### Fixed

- The server delay test and the automatic re-ping after a subscription update no longer reset the selected server
- Subscription updates no longer interrupt active connections: the config is reloaded softly unless ports/TUN actually changed
- The routing mode (Rules/Global/Direct) no longer flips back to Rules after picking Global in the settings

### Changed

- Starred servers no longer take over the selection: they float to the top of the list and are used automatically only when the selected server is missing or stops responding
- Every link header from the panel is now accepted over https only
- Device identity headers are koala-clash style: plain `ClodClash/<version>` User-Agent, human-readable OS version and system edition instead of the computer name
- The simple home screen now shows the connect targets (system proxy / TUN) and the active routing mode under the Connect button

<!-- lang:ru -->

### Исправлено

- Тест серверов и автоперепинговка после обновления подписки больше не сбрасывают выбранный сервер
- Обновление подписки не рвёт активные соединения: конфиг перезагружается мягко, если порты/TUN не менялись
- Режим маршрутизации (Правила/Глобальный/Прямой) больше не слетает обратно на «Правила» после выбора «Глобальный»

### Изменено

- Избранные серверы не перехватывают выбор: они вверху списка и подхватываются автоматически, только когда выбранный сервер пропал или перестал отвечать
- Все ссылки из заголовков панели принимаются только по https
- Идентификация устройства как у koala-clash: User-Agent «ClodClash/версия», человекочитаемая версия ОС и редакция системы вместо имени компьютера
- В простом режиме под кнопкой Connect видно, что включает Connect (системный прокси / TUN) и какой режим маршрутизации активен

---

## v0.0.8-alpha

### Added

- Starred servers win: on app start and after a subscription update a starred server is picked automatically wherever no explicit choice exists

### Fixed

- The selected server no longer resets after a subscription update or profile reactivation
- Banner links (`announce-url`, `clod-promo-url`) are now accepted only over https

---

## v0.0.7-alpha

### Fixed

- The update dialog opened empty («New Version v», no changelog, dead Update button) when automatic update checks were disabled — it now fetches update info on its own whenever it opens

---

## v0.0.6-alpha

### Changed

- Update check lives on the **Clod Version** row now (gear icon, like the core row); the separate "Check for updates" entry is gone
- Every home tile got its themed background icon — vertically centred at the right edge instead of bleeding out of a corner
- Subscription updates log which panel headers were present (easier remote diagnostics)

---

## v0.0.5-alpha

### Fixed

- Servers appear right after adding a subscription — no manual refresh needed
- Latency values re-measure automatically after a subscription update instead of disappearing
- Turning the proxy on no longer makes scrollbars appear in simple mode
- Russian labels no longer overflow their buttons

### Changed

- The Network card shows speeds and downloaded/uploaded totals instead of a graph
- The subscription block is now two matching tiles — traffic and expiry — with themed background icons
- Windows CI builds got faster (relaxed release codegen for alpha builds)

---

## v0.0.4-alpha

### Fixed

- The **Update** button in the new-version dialog did nothing when the release notes were empty
- **Go to Release Page** opened a wrong (upstream) repository
- Removed the internal `COMPATIBLE` placeholder from server names on the home screen

### Changed

- Subscriptions page cleaned up: only your subscriptions, with support and account buttons on each card
- Settings reorganized into a single column — everyday options on top, advanced ones below
- IPv6 is now disabled by default on desktop
- The update dialog now shows this changelog

---

## v0.0.3-alpha

### Added

- Server picker with proxy-group chips in a row
- Latency shown right on the home screen server row
- Session traffic counters (downloaded / uploaded since start)
- The window remembers its size separately for simple and advanced mode

### Changed

- Fewer tiles on the advanced home screen; technical sections moved to Settings

---
