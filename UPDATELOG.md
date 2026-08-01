# Update Log

The bilingual changelog. Each `## v{version}` section (ended by `---`)
contains an English part and a Russian part separated by `<!-- lang:en -->`
and `<!-- lang:ru -->` markers. The release workflow injects the whole
section into `latest.json` (updater notes) and into the GitHub release
body; the app's update dialog picks the part matching the UI language
(Russian UI → ru, anything else → en). Sections without markers are shown
as-is.

## v0.0.12-alpha

<!-- lang:en -->

### Added

- Empty server lists now explain themselves: an expired subscription, exhausted traffic or a panel that sent no servers each get their own status with the right buttons (Renew, Top up, Support) — on the home screen and in the server drawer
- Placeholder "nodes" a panel sends instead of servers (0.0.0.0 stubs) are filtered out before they reach the core, so the app no longer pretends they are servers you could connect to
- Providers can colour single words in `announce` and promo banners with `#RRGGBB` markers glued to a word
- A "Device identification" switch in Settings → General, with a tooltip showing exactly what is sent to the panel
- The provider logo is cached locally: it survives restarts, works offline and is fetched through the tunnel
- Each proxy group is tested against its own `url:` from the config — the YouTube group is checked against YouTube, not a generic endpoint
- "Copy report for support" under every error and in Settings: log tails with visited addresses removed and tokens, keys and URLs masked

### Fixed

- A panel demanding device identification can no longer wipe a working server list with its stub response
- The "no servers" explanation always describes the config the core actually accepted, not one it rejected
- Group test URLs no longer leak between profiles after a subscription switch
- Redirects to IPv6 local addresses are refused when fetching the provider logo

<!-- lang:ru -->

### Добавлено

- Пустой список серверов теперь объясняет себя: истёкшая подписка, исчерпанный трафик и «панель не выдала серверы» получили свои статусы с нужными кнопками («Продлить», «Докупить», «Поддержка») — на главной и в шторке серверов
- Узлы-заглушки, которые панель шлёт вместо серверов (0.0.0.0), вырезаются до ядра — приложение больше не выдаёт их за серверы, к которым можно подключиться
- Провайдер может красить отдельные слова в `announce` и промо-баннере маркерами `#RRGGBB`, приклеенными к слову
- Тумблер «Идентификация устройства» в Настройки → Основные, с тултипом, показывающим, что именно уходит панели
- Логотип провайдера кэшируется локально: переживает перезапуск, работает офлайн и скачивается через туннель
- Каждая группа прокси проверяется по своему `url:` из конфига — YouTube-группа по YouTube, а не по общему адресу
- «Скопировать отчёт для поддержки» под каждой ошибкой и в настройках: хвосты логов без посещённых адресов, токены, ключи и URL замаскированы

### Исправлено

- Панель, требующая идентификацию устройства, больше не может затереть рабочий список серверов своей заглушкой
- Объяснение «серверов нет» всегда описывает конфиг, который ядро реально приняло, а не отвергнутый
- Адреса теста групп больше не перетекают между профилями при смене подписки
- Редиректы на локальные IPv6-адреса отклоняются при скачивании логотипа провайдера

---

## v0.0.11-alpha

<!-- lang:en -->

### Fixed

- The update dialog no longer shows a stale empty "New version" screen: opening it always runs a fresh check instead of reusing the result cached at app start
- A found update now announces itself — the update dialog opens automatically (once per version per app run); previously the only notification lived in a hidden sidebar
- The app re-checks for updates every 3 hours while running instead of once a day
- Server list polish that missed 0.0.10: the active entry says "In use", a failed delay test shows a dash instead of "1000000 ms", group chips carry the country flag of the resolved node

<!-- lang:ru -->

### Исправлено

- Окно обновления больше не показывает пустую «Новую версию»: каждое открытие запускает свежую проверку вместо результата, закэшированного при старте приложения
- Найденное обновление теперь само даёт о себе знать — окно обновления открывается автоматически (один раз за версию на запуск); раньше единственное уведомление жило в скрытой боковой панели
- Работающее приложение перепроверяет обновления каждые 3 часа, а не раз в сутки
- Полировка списка серверов, не попавшая в 0.0.10: активная строка подписана «Используется», проваленный тест показывает прочерк вместо «1000000 мс», у чипов групп — флаг страны выбранного узла

---

## v0.0.10-alpha

<!-- lang:en -->

### Fixed

- The traffic progress bar no longer runs under the tile's background icon
- The simple home screen fits its window again — no stray scrollbar
- The refresh button in the header now confirms "Subscription updated" just like the advanced-mode tile

### Changed

- Changelogs and GitHub releases are now bilingual; the update dialog shows Russian for the Russian UI and English otherwise

<!-- lang:ru -->

### Исправлено

- Полоска трафика больше не заезжает под фоновую иконку плитки
- Простой режим снова помещается в окно — лишняя прокрутка исчезла
- Кнопка обновления в шапке теперь показывает «Подписка обновлена», как плитка в расширенном режиме

### Изменено

- Ченджлоги и GitHub-релизы теперь двуязычные; окно обновления показывает русский текст при русском интерфейсе и английский в остальных случаях

---

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
