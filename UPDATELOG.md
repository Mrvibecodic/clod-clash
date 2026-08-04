# Update Log

The bilingual changelog. Each `## v{version}` section (ended by `---`)
contains an English part and a Russian part separated by `<!-- lang:en -->`
and `<!-- lang:ru -->` markers. The release workflow injects the whole
section into `latest.json` (updater notes) and into the GitHub release
body; the app's update dialog picks the part matching the UI language
(Russian UI → ru, anything else → en). Sections without markers are shown
as-is.

## v0.0.16-alpha

<!-- lang:en -->

### Changed

- One toggle style everywhere. The bulky outlined switch crowded the rows of every settings list and looked foreign in dialogs; the app now uses the compact system toggle, with only its off state strengthened so it stays visible on a white card
- The app no longer asks twice what the Connect button switches. The "Connect: system proxy" and "Connect: TUN mode" settings duplicated a choice you already make with the live switches, so they are gone: whichever mode you turn on by hand is the one Connect brings up next time
- Logs, the support report and the Windows installer speak Russian instead of Chinese — nearly seven hundred leftover upstream messages, including the ones the installer prints while it runs

### Fixed

- The subscription screen no longer keeps a dead uptime request alive: the app polled the backend every three seconds for a value nothing displayed any more

<!-- lang:ru -->

### Изменено

- Один вид тумблера на всё приложение. Крупный переключатель с обводкой занимал почти всю строку в списках настроек и выглядел чужеродно в диалогах — теперь везде компактный системный тумблер, у которого усилено только выключенное состояние, чтобы он не терялся на белой карточке
- Приложение больше не спрашивает дважды, что включает кнопка Connect. Настройки «Подключение: системный прокси» и «Подключение: режим TUN» дублировали выбор, который вы уже делаете живыми тумблерами, и удалены: какой режим включили руками — тот Connect и поднимет в следующий раз
- Логи, отчёт для поддержки и установщик Windows говорят по-русски, а не по-китайски: без малого семьсот сообщений, доставшихся от апстрима, включая те, что установщик печатает по ходу установки

### Исправлено

- Приложение перестало опрашивать бэкенд каждые три секунды ради времени работы, которое давно негде показывать

---

## v0.0.15-alpha

<!-- lang:en -->

### Fixed

- The app no longer asks for administrator rights over and over after an update. The service check used to treat "the service on disk is older than the app" as "reinstall it now" — and that check runs on every core start, which on Windows is retried five times, so a single update could raise the elevation prompt a dozen times in a row. It now only looks: one notice per session saying the service needs repair, and repairing happens when you press the button
- The TUN switch no longer promises what it cannot deliver: a service left over from a previous version answers, but the core cannot start through it, so the app now checks that the versions match and offers to repair rather than looping on "TUN did not start"
- A subscription profile can no longer switch TUN off — or on — behind your back: the `tun` section belongs to the app and is restored after any manual override or provider profile
- macOS: the system DNS is restored even if the app was killed. The original value is remembered once, together with the network service it belongs to, and put back on the next start; the override is also no longer re-applied on every config change
- Subscriptions are fetched through the tunnel when the direct route fails — for imports too, not just refreshes. Adding a subscription whose domain is blocked now works as long as the app is connected
- The "New version" window fits the simple mode again: the width is computed from the window, the release button moved out of the title, and long lines wrap instead of pushing a horizontal scrollbar

<!-- lang:ru -->

### Исправлено

- Приложение больше не просит права администратора снова и снова после обновления. Проверка службы считала «служба на диске старее приложения» поводом немедленно её переустановить — а проверка эта выполняется при каждом старте ядра, который на Windows повторяется до пяти раз, поэтому одно обновление выкатывало запрос прав по десятку раз подряд. Теперь проверка только смотрит: одно уведомление за сессию о том, что службу надо починить, а чинится она по нажатию кнопки
- Тумблер TUN больше не обещает невозможного: служба от прошлой версии отвечает, но ядро через неё не поднимается — приложение проверяет совпадение версий и предлагает ремонт вместо бесконечного «TUN не запустился»
- Профиль подписки больше не может выключить (или включить) TUN за вашей спиной: секция `tun` принадлежит приложению и восстанавливается после любых ручных правок и профилей провайдера
- macOS: системный DNS возвращается, даже если приложение убили. Исходное значение запоминается один раз вместе с именем сетевого сервиса и возвращается при следующем запуске; подмена больше не повторяется при каждом изменении конфигурации
- Подписка загружается через туннель, когда прямой путь не работает, — теперь и при импорте, а не только при обновлении. Добавить подписку с заблокированным доменом можно, пока приложение подключено
- Окно «Новая версия» снова помещается в простом режиме: ширина считается от окна, кнопка релиза переехала из заголовка, а длинные строки переносятся вместо горизонтальной прокрутки

---

## v0.0.14-alpha

<!-- lang:en -->

### Added

- Adding a subscription takes one field now: paste the link your service gave you and press Add. Name, group, refresh interval, User-Agent, timeout and the switches moved into a folded "Advanced" block — when editing an existing subscription it opens right away, because that is what you came for
- After adding, the window shows what the link resolved to — name, traffic and expiry — so you can see the right subscription arrived before closing it
- Subscription groups: give a subscription a label in its properties (a new group is created right there) and a filter row appears above the grid with a count per group. An empty group disappears by itself
- The subscription cards say what state they are in: the active one is filled with the accent colour and labelled "Active", an expiring one is amber, an expired one is dimmed and underlined in red, and traffic exhausted while the plan is still valid is its own state

### Fixed

- Switches that are off no longer blend into the background in the light theme: they now have an outline instead of a pale grey bar. The quick actions on the home screen use the same switch as everywhere else
- The core is polled for traffic far less often. How often follows the subscription's own refresh interval — with an hourly refresh the local estimate is not needed at all and is turned off, and beyond that the interval scales up to five minutes instead of the previous five seconds
- Errors while adding a subscription stay in the window next to the field instead of flying off as a toast, and what you typed is not lost

<!-- lang:ru -->

### Добавлено

- Подписка добавляется одним полем: вставьте ссылку, которую выдал ваш сервис, и нажмите «Добавить». Название, группа, интервал обновления, User-Agent, таймаут и тумблеры уехали в свёрнутый блок «Дополнительно» — при правке существующей подписки он раскрыт сразу, потому что заходят туда именно за ним
- После добавления окно показывает, что нашлось по ссылке — название, трафик и срок, — чтобы было видно, та ли подписка пришла, ещё до закрытия
- Группы подписок: поставьте подписке ярлык в её свойствах (там же создаётся новая группа), и над сеткой появится ряд фильтров с числом карточек в каждой. Пустая группа исчезает сама
- Карточки подписок говорят, в каком они состоянии: активная залита цветом и подписана «Активна», истекающая — жёлтым, истёкшая приглушена и подчёркнута красным, а исчерпанный трафик при живом сроке — отдельное состояние

### Исправлено

- Выключенные тумблеры больше не сливаются с фоном на светлой теме: вместо бледной серой полосы у них теперь контур. Быстрые действия на главной используют тот же тумблер, что и все остальные экраны
- Ядро опрашивается ради трафика заметно реже. Частота идёт от интервала обновления самой подписки: при обновлении раз в час местный досчёт не нужен и выключается, а дальше интервал растёт до пяти минут вместо прежних пяти секунд
- Ошибка при добавлении подписки остаётся в окне рядом с полем, а не улетает уведомлением в угол, и введённое не теряется

---

## v0.0.13-alpha

<!-- lang:en -->

### Added

- TUN mode sets itself up: on the first launch, once the window is up, the app installs the background helper it needs with a single elevation prompt — for the installer alone, the app itself stays unprivileged. Refuse it and the app keeps working through the system proxy without ever nagging again; the TUN switch still installs it on demand, because then you are the one asking
- A standing line under the switches (under the Connect button in the simple mode) explains what is happening: "Setting TUN up — confirm the system prompt" while the helper is installed, and "TUN did not start" with a "Set up" button if it failed. A toast disappears in five seconds; this does not
- Quick actions on the advanced home screen: system proxy, TUN, start with the system and start minimized, all without opening the settings
- The server drawer grows to fit the group instead of scrolling a fixed strip, up to the bottom edge of the Connect button
- Traffic used between subscription refreshes is counted locally and shown as an estimate (≈) — panels report their number once an hour, so the card no longer looks frozen

### Fixed

- The app no longer erases your TUN choice: the check that wrote "TUN off" straight into the config ran before the helper could answer, which on an autostart it never did in time. Unavailability is now scoped to the running session
- The TUN switch shows what is actually running, not what the config asks for — it can no longer glow over a dead tunnel
- A helper that comes up slower than the app is now waited for on macOS and Linux too, and the core moves over to it without dropping connections
- A core that fails to start through the helper falls back to the built-in one instead of leaving you with no core at all
- A core that dies on its own is restarted, up to three times in a row
- The error under the Connect button no longer stays red after the problem is fixed

<!-- lang:ru -->

### Добавлено

- Режим TUN настраивается сам: при первом запуске, уже после появления окна, приложение ставит нужную ему фоновую службу — один запрос прав, и только для установщика, само приложение остаётся без привилегий. Откажетесь — приложение продолжит работать через системный прокси и больше не будет спрашивать; тумблер TUN при этом рабочий: нажали — поставит, потому что теперь просите вы
- Постоянная строка под переключателями (в простом режиме — под кнопкой подключения) объясняет происходящее: «Настраиваем TUN — подтвердите запрос системы», пока идёт установка, и «TUN не запустился» с кнопкой «Настроить», если не вышло. Уведомление исчезает через пять секунд, строка — нет
- Карточка быстрых действий на главной расширенного режима: системный прокси, TUN, запуск с системой и старт свёрнутым — без захода в настройки
- Шторка выбора сервера растёт под размер группы, а не прокручивает узкую полосу, — до нижней кромки кнопки подключения
- Расход трафика между обновлениями подписки досчитывается на месте и показывается как примерный (≈): панель отдаёт своё число раз в час, и карточка больше не выглядит замершей

### Исправлено

- Приложение больше не стирает ваш выбор TUN: проверка, писавшая «TUN выключен» прямо в конфиг, выполнялась раньше, чем служба успевала ответить, — а при автозапуске она не успевала никогда. Недоступность теперь действует только до перезапуска
- Тумблер TUN показывает то, что работает, а не то, что записано в настройках, — гореть над мёртвым туннелем он больше не может
- Службу, которая поднимается медленнее приложения, теперь ждут и на macOS с Linux, а ядро переезжает на неё без разрыва соединений
- Ядро, не сумевшее запуститься через службу, откатывается на встроенный запуск вместо того, чтобы оставить вас вообще без ядра
- Упавшее ядро перезапускается — до трёх попыток подряд
- Ошибка под кнопкой подключения больше не остаётся красной после того, как причина устранена

---

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
