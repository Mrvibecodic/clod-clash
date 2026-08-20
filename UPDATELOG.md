# Update Log

The bilingual changelog. Each `## v{version}` section (ended by `---`)
contains an English part and a Russian part separated by `<!-- lang:en -->`
and `<!-- lang:ru -->` markers. The release workflow injects the whole
section into `latest.json` (updater notes) and into the GitHub release
body; the app's update dialog picks the part matching the UI language
(Russian UI → ru, anything else → en). Sections without markers are shown
as-is.

## v0.1.5

<!-- lang:en -->

### Fixed

- Provider links with `tg://` addresses (support, bot and the rest) now open in the Telegram app; a link that cannot be opened shows a message instead of failing silently
- The «Fit window to content» setting survives an app restart and update: window moves during startup are no longer taken for a manual resize
- The speed graph is removed from the «Network» card — it is four numbers again
- The last-update hint on the «Refresh subscription» tile no longer gets cut off: it shows just the date and time

<!-- lang:ru -->

### Исправлено

- Ссылки провайдера с адресами `tg://` (поддержка, бот и остальные) открываются в приложении Telegram; если ссылку открыть не удалось, показывается сообщение, а не тишина
- Настройка «Подгонять окно под содержимое» переживает перезапуск и обновление приложения: движения окна при старте больше не принимаются за ручное изменение размера
- Из карточки «Сеть» убран график скорости — снова четыре числа
- Подсказка о последнем обновлении на плитке «Обновить подписку» больше не обрезается: показываются только дата и время

---

## v0.1.4

<!-- lang:en -->

### Added

- Windows: the installer now adds firewall rules for the core, so the `system` and `mixed` TUN stacks work without extra steps; uninstalling removes the rules
- Windows: if inbound connections for the core are still blocked, the home screen shows a warning with a «Fix» button — one click and one admin prompt
- The «Network» card draws a small graph of the recent download and upload speed
- The «Refresh subscription» tile shows when the subscription was last updated

### Changed

- A refreshed look: cards sit on soft shadows instead of hard borders, a subtle accent gradient at the top of the window, pill-shaped buttons, card titles without all-caps
- The row under the connect button became a pill with icons: what Connect drives and the routing mode
- Quick actions got icons and the «Connection» / «Startup» groups; TUN and firewall warnings are shown as highlighted panels with the action right inside
- The connect button glows green while connected; the toggles are larger, with the knob inside the track; cards lift slightly under the cursor

<!-- lang:ru -->

### Добавлено

- Windows: установщик добавляет правила брандмауэра для ядра — стеки TUN `system` и `mixed` работают без лишних действий; при удалении приложения правила снимаются
- Windows: если входящие подключения ядра всё же заблокированы, на главной появляется предупреждение с кнопкой «Починить» — один клик и один запрос прав
- Карточка «Сеть» рисует небольшой график скорости загрузки и отдачи за последние минуты
- Плитка «Обновить подписку» показывает, когда подписка обновлялась в последний раз

### Изменено

- Обновлённый вид: карточки на мягких тенях вместо жёстких рамок, лёгкий градиент акцента сверху окна, кнопки-пилюли, заголовки карточек без капса
- Строка под кнопкой подключения стала пилюлей с иконками: что включает Connect и режим маршрутизации
- У быстрых действий появились иконки и группы «Подключение» / «Запуск»; предупреждения TUN и брандмауэра — заметные плашки с кнопкой прямо в них
- Кнопка подключения светится зелёным, пока соединение активно; тумблеры крупнее, с бегунком внутри трека; карточки слегка приподнимаются под курсором

---

## v0.1.3

<!-- lang:en -->

### Added

- TUN: the tunnel is now taken down by the app itself when the connection button is off, and is recreated at once if the tunnel is switched back on meanwhile
- After the tunnel comes up its traffic is probed: if the local proxy answers but the direct path stays silent, a notice appears — on Windows with a hint about the firewall for the `system` and `mixed` stacks
- The TUN dialog shows the stack the core actually applied, so with «Auto» it is visible what the subscription chose, and warns about `system` and `mixed` on Windows
- Builds for Intel Macs: the release now carries an `x86_64` build next to the Apple Silicon one

### Fixed

- Windows: the app no longer freezes after the window is restored from the tray — debug tracing was getting into release builds and turned every event sent to the page into a wait on the main thread
- Quitting now waits for the tunnel to really come down and the core to stop, so routes are not left hanging without internet until the next start
- Windows: a core that survived a crash is swept away on start instead of living on with a raised tunnel and an old config
- Windows: a busy service channel is no longer read as «no service» — that dropped the core into the unprivileged mode and the tunnel silently did not come up

<!-- lang:ru -->

### Добавлено

- TUN: туннель гаснет сам, если кнопка подключения выключена, и сразу пересоздаётся, если за это время туннель успели включить обратно
- После подъёма туннеля проверяется трафик: если локальный прокси отвечает, а прямой путь молчит, появляется уведомление — на Windows с подсказкой про брандмауэр для стеков `system` и `mixed`
- Диалог TUN показывает стек, фактически применённый ядром: при «Авто» видно, что выбрала подписка, — и предупреждает о выборе `system` или `mixed` на Windows
- Сборка под маки с процессором Intel: в релизе рядом с версией для Apple Silicon появилась `x86_64`

### Исправлено

- Windows: приложение больше не зависает после разворачивания окна из трея — отладочная трассировка попадала в боевую сборку и превращала отправку каждого события странице в ожидание главного потока
- Выход из приложения ждёт, пока туннель действительно снимется, а ядро остановится, — маршруты больше не остаются висеть без интернета до следующего запуска
- Windows: ядро, пережившее аварийное завершение, подметается при старте, а не живёт дальше с поднятым туннелем и старым конфигом
- Windows: занятый канал службы больше не читается как «службы нет» — из-за этого ядро уходило в режим без прав и туннель молча не поднимался

---

## v0.1.2

<!-- lang:en -->

### Added

- TUN: the network stack, strict routing and DNS interception can now follow the subscription («Auto») or be set by hand in Settings → TUN
- A provider can hide latency numbers with the `clod-disable-ping` header: a server then shows a green check, a red cross or a dash
- The logs screen got a «Save to file» button — it writes exactly what is on screen, with the current level, search and order

### Fixed

- The tunnel is raised again after a failed start, and comes back after sleep or a network change
- A registered but stopped background service is started without asking for rights (Windows)
- Linux: the service is no longer removed when the package is updated, so the password is not asked again
- After a crash no orphaned core stays running (Linux, macOS)
- A subscription link opens the app and imports the subscription even when the app was closed or hidden in the tray, and links with `&` or `#` are no longer cut short
- The right click shows the system menu in text fields again (Windows)
- Connections really close when the server changes and the matching setting is on
- Custom theme CSS is not lost when the settings window is closed
- Checking for updates by hand goes through the local proxy when GitHub is not reachable directly

<!-- lang:ru -->

### Добавлено

- TUN: сетевой стек, строгая маршрутизация и перехват DNS теперь могут следовать за подпиской («Авто») или задаваться вручную в «Настройки → TUN»
- Провайдер может скрыть цифры пинга заголовком `clod-disable-ping`: вместо миллисекунд у сервера появляется зелёная галочка, красный крестик или прочерк
- На экране логов появилась кнопка «Сохранить в файл» — пишет ровно то, что на экране, с учётом уровня, поиска и порядка

### Исправлено

- Туннель поднимается повторно после неудачного старта и восстанавливается после сна и смены сети
- Зарегистрированная, но остановленная фоновая служба запускается без запроса прав (Windows)
- Linux: служба больше не удаляется при обновлении пакета, поэтому пароль не спрашивают заново
- После аварийного завершения не остаётся запущенное ядро (Linux, macOS)
- Ссылка на подписку открывает приложение и добавляет подписку, даже если оно было закрыто или свёрнуто в трей, а ссылки с `&` и `#` больше не обрезаются
- Правый клик в полях ввода снова показывает системное меню (Windows)
- Соединения действительно закрываются при смене сервера, когда включена соответствующая настройка
- Свой CSS темы не теряется при закрытии окна настроек
- Проверка обновлений вручную проходит через локальный прокси, если GitHub напрямую недоступен

---

## v0.1.1

<!-- lang:en -->

### Fixed

- After the core updates itself, the app starts it through the system service again instead of falling back and losing TUN
- A refused core start no longer produces five identical error notifications in a row

<!-- lang:ru -->

### Исправлено

- После обновления ядра приложение снова запускает его через системную службу, а не откатывается в обычный режим с потерей TUN
- Отказ в запуске ядра больше не выдаёт пять одинаковых уведомлений подряд

---

## v0.1.0

<!-- lang:en -->

### Added

- Shortcuts to the tools on the home screen: Proxies, Rules, Connections and Logs now appear as tiles right under «Subscriptions / Refresh / Settings» in the advanced interface. Each one is turned on or off in Settings → Advanced settings → Tools; all four are on, and a tool switched off simply leaves the row — with none left the row disappears entirely

### Fixed

- After an update the desktop and Start menu shortcuts show the current icon instead of the old one
- The tray icon matches the new app icon in every state — plain, system proxy and TUN, in both the colour and the monochrome set
- The home screen no longer flashes the «add a subscription» prompt for a moment at startup
- If the window stops responding after being restored from the taskbar, the app now notices it and writes down what it was doing, instead of going quiet

<!-- lang:ru -->

### Добавлено

- Ярлыки инструментов на главном экране: Прокси, Правила, Соединения и Логи встали плитками прямо под «Подписки / Обновить / Настройки» в расширенном режиме. Каждый включается и выключается в «Настройки → Продвинутые настройки → Инструменты»; включены все четыре, выключенный просто уходит из ряда, а если не осталось ни одного — ряда нет вовсе

### Исправлено

- После обновления ярлыки на рабочем столе и в «Пуске» показывают текущий значок, а не прежний
- Значок в трее совпадает с новой иконкой приложения во всех состояниях — обычном, системного прокси и TUN, в цветном и монохромном наборе
- Главный экран больше не мигает на запуске приглашением добавить подписку
- Если окно перестаёт отвечать после разворачивания из панели задач, приложение замечает это и записывает, на чём остановилось, вместо того чтобы молчать

---

## v0.0.31-alpha

<!-- lang:en -->

### Changed

- A visual polish pass across the whole app: one border colour instead of two different ones, one corner radius scale, one motion curve — cards, tiles and rows now respond to the pointer smoothly instead of snapping
- Switching between light and dark theme cross-fades instead of flashing
- Dialogs, menus and the server drawer visibly lift above the page again; the selected subscription card got its highlight ring back
- Keyboard navigation is finally visible: an accent-coloured focus ring follows the Tab key. Mouse clicks stay clean, with no rings
- In the dark theme every panel is the same shade — cards, node rows and dialogs no longer disagree with each other; accent colours are lifted slightly so they stay readable on dark
- Numbers in traffic, network and latency readouts use tabular digits and no longer twitch as they update; log levels became small badges
- A new app icon: the same robot, redrawn on a strict grid — larger head, crisper at small sizes, a smoother indigo-violet gradient

<!-- lang:ru -->

### Изменено

- Визуальный проход по всему приложению: одна линия вместо двух разных, одна шкала скруглений, одна кривая движения — карточки, плитки и строки отвечают на курсор плавно, а не рывком
- Светлая и тёмная тема переключаются кросс-фейдом, без вспышки
- Диалоги, меню и шторка серверов снова заметно отрываются от страницы; к выбранной подписке вернулось кольцо выделения
- Навигацию с клавиатуры теперь видно: за Tab следует кольцо цвета акцента. Мышью — как раньше, без рамок
- В тёмной теме все панели одного оттенка — карточки, строки узлов и диалоги больше не спорят друг с другом; акцентные цвета чуть подняты, чтобы читались на тёмном
- Цифры трафика, сети и задержек стали табличными и не дёргаются при обновлении; уровни в логах превратились в аккуратные бейджи
- Новая иконка приложения: тот же робот, перерисованный по строгой сетке — крупнее голова, чётче на мелких размерах, ровнее градиент индиго-фиолетовый

---

## v0.0.30-alpha

<!-- lang:en -->

### Fixed

- The app no longer hangs when the window is restored after sitting minimised for hours. Windows was putting the web view to sleep, and waking the window up waited for an answer that never came — the last frame stayed on screen and the window said "Not responding" for good
- The interface language stops falling back to the system one. The choice now lives in the app settings and is fixed on the first launch, instead of depending on browser storage that an update wipes out — this was mostly seen on Linux

### Added

- Provider links now sit in one row of their own, signed with the provider's name: account, support, bot, server status and the setup guide. The tiles below stay what they were — the app's own actions
- The same links are duplicated in the settings, in a card that belongs to the current subscription. Links of another subscription never mix in
- Three new panel headers behind those buttons: `clod-bot-url`, `clod-monitor-url`, `clod-guide-url`. A header the panel stops sending removes its button

### Changed

- The "General" settings group is collapsed by default, like "Advanced" already was: language and theme are set once

<!-- lang:ru -->

### Исправлено

- Приложение больше не зависает, когда окно разворачивают после нескольких часов в свёрнутом виде. Windows усыпляла вебвью, и показ окна ждал ответа, которого уже не будет: на экране оставался последний кадр, а окно навсегда становилось «Не отвечает»
- Язык интерфейса перестал сбрасываться на системный. Выбор теперь живёт в настройках приложения и закрепляется с первого запуска, а не зависит от хранилища вебвью, которое обновление стирает — чаще всего это ловилось на Linux

### Добавлено

- Ссылки провайдера собраны в отдельную строку, подписанную его именем: кабинет, поддержка, бот, мониторинг серверов и инструкция. Плитками ниже остались действия самого приложения
- Те же ссылки продублированы в настройках отдельной карточкой, принадлежащей текущей подписке. Ссылки другой подписки в неё не попадают
- Под эти кнопки заведены три заголовка панели: `clod-bot-url`, `clod-monitor-url`, `clod-guide-url`. Пропал заголовок — пропала и кнопка

### Изменено

- Группа настроек «Основные» свёрнута по умолчанию, как уже были свёрнуты «Продвинутые»: язык и тему задают один раз

---

## v0.0.29-alpha

<!-- lang:en -->

### Fixed

- The refresh button on a subscription card works again. The status badge was drawn on top of it, so on the subscription you are actually using the tap never reached the button

### Changed

- Subscription cards were rebuilt: the name, the state badge, the address and the update time each get their own line instead of fighting for one, the device-limit warning is no longer cut off, and a card is never narrower than 320 px — the grid fits as many per row as the window allows

### Added

- The "Secure connection" checkbox now sits right under the link field, both in the add-subscription dialog and on the welcome screen. The choice cannot be undone later, so it should not be hidden behind "Advanced"
- A protected subscription is marked with a shield on its card

<!-- lang:ru -->

### Исправлено

- Кнопка обновления на карточке подписки снова работает. Поверх неё рисовался бейдж состояния, поэтому на той подписке, которой вы пользуетесь, нажатие до кнопки не доходило

### Изменено

- Карточка подписки перебрана: название, состояние, адрес и время обновления получили по отдельной строке вместо борьбы за одну, предупреждение о лимите устройств больше не обрезается, а сама карточка не бывает уже 320 px — в ряд помещается столько, сколько позволяет окно

### Добавлено

- Галочка «Защищённое соединение» теперь стоит прямо под полем ссылки — и в окне добавления подписки, и на приветственном экране. Выбор потом не отменить, поэтому прятать его в «Дополнительно» неправильно
- Защищённая подписка помечена щитом на карточке

---

## v0.0.28-alpha

<!-- lang:en -->

### Fixed

- Installing the background service on systems with SELinux (Fedora, RHEL) no longer fails on the first attempt. The place the service binary is copied to is labelled before the installer runs, so the system never denies the start in the first place — instead of a SELinux alert, an error, and a service that came up only on the second try

<!-- lang:ru -->

### Исправлено

- Установка фоновой службы на системах с SELinux (Fedora, RHEL) больше не срывается с первой попытки. Каталог, куда кладётся бинарь службы, размечается до запуска установщика, поэтому система не запрещает запуск вовсе — вместо алерта SELinux, ошибки и службы, поднявшейся только со второго раза

---

## v0.0.27-alpha

<!-- lang:en -->

### Added

- The window now fits its content: nothing on the home screen hides behind a scrollbar any more. It grows when a banner arrives and settles back when it goes, up to what the screen has room for; resizing the window by hand hands the size back to you, and the "Fit window to content" switch in the settings hands it back to the app
- A long promotional banner is folded to five lines with a "Show in full" button; the full text opens over the window

### Fixed

- The background service required for TUN installs on systems with SELinux (Fedora, RHEL): the service binary is labelled so systemd may start it, and it is done within the same permission prompt. When something still blocks it, the error says so and gives the command to fix it
- The selected server survives a core restart — after a crash, and after the app hands the core over to the background service, which on Windows with TUN is the usual way it starts. The tray and the proxy chain now remember the choice too, and two quick switches no longer overwrite each other
- Deleting a subscription is undone if the configuration cannot be built without it. The files are removed last, once the app has proved it still works
- A profile file that is not valid YAML is no longer overwritten with an empty one by the node and group editors: it opens as text and says what happened. A node without a name no longer takes the whole editor down with it
- Automatic updates on Windows no longer stop at the installer's language dialog hidden behind the update splash
- A subscription refresh is no longer cut off halfway on a slow panel, and waiting for the background service at startup has an upper bound
- The log stream no longer dies quietly when the first request after connecting fails
- Failures to set the system proxy say what the system actually reported instead of "system call failed"
- The port-in-use check asks about the address the core will really listen on, so a busy port is no longer reported as free
- Events sent from background work go through the main thread, closing a class of hangs
- The encrypted subscription channel pads every request to the same size and retries without a pinned key when the relay is replaced

<!-- lang:ru -->

### Добавлено

- Окно подстраивается под содержимое: на главном экране больше ничего не прячется за прокруткой. Оно подрастает, когда приходит баннер, и садится обратно, когда тот уходит, — насколько позволяет экран. Изменение размера мышью возвращает размер вам, а тумблер «Подгонять окно под содержимое» в настройках возвращает его приложению
- Длинный рекламный баннер сворачивается до пяти строк с кнопкой «Показать полностью»; полный текст открывается поверх окна

### Исправлено

- Фоновая служба, нужная для TUN, устанавливается на системах с SELinux (Fedora, RHEL): бинарю службы проставляется метка, с которой systemd имеет право его запускать, и делается это в том же запросе прав. Если запуску мешает что-то ещё, в ошибке теперь написано что именно и как это поправить
- Выбранный сервер переживает перезапуск ядра — и после падения, и после передачи ядра фоновой службе, а на Windows с TUN это обычный путь запуска. Трей и цепочка прокси тоже запоминают выбор, а два быстрых переключения подряд больше не затирают друг друга
- Удаление подписки отменяется, если конфиг без неё не собрался. Файлы стираются последними, когда приложение уже доказало, что работает
- Файл профиля, который не разбирается как YAML, больше не затирается пустым в редакторах нод и групп: он открывается как текст и объясняет, что случилось. Нода без имени больше не роняет весь редактор
- Автообновление на Windows не останавливается на диалоге выбора языка установщика, спрятанном за заставкой обновления
- Обновление подписки не обрывается на середине на медленной панели, а ожидание фоновой службы при запуске ограничено по времени
- Поток логов больше не умирает молча, когда первый запрос после подключения не удался
- Ошибки установки системного прокси показывают, что на самом деле ответила система, вместо «system call failed»
- Проверка занятости порта спрашивает тот адрес, на котором ядро действительно будет слушать, — занятый порт больше не выглядит свободным
- События из фоновой работы отправляются через главный поток, что закрывает целый класс подвисаний
- Зашифрованный канал подписки дополняет каждый запрос до одного размера и повторяет попытку без закреплённого ключа, когда прослойку заменили

---

## v0.0.26-alpha

<!-- lang:en -->

### Added

- A subscription can be fetched over an encrypted channel. The address carries nothing readable, the reply looks like any other response and every request is padded to the same size. It is off by default and switched on per subscription; the relay key is pinned the first time it is seen, and its fingerprint is shown next to the switch
- "Connect on launch" — the app can repeat the Connect press right after it starts. Off by default

### Changed

- Nothing is written into the system proxy settings until a connection method is chosen and a connection is asked for. An app that was merely started, or one with no subscription at all, no longer configures the system on its own; deleting the last subscription clears the setting at once
- The settings screen follows the connection method the provider fixed: the method that is not part of it is not shown at all, and the one that is says who chose it. A method left over from an earlier session is switched off, in the system settings too
- The advanced window is wider by default, and its content scrolls instead of being squeezed: the network card and the tiles no longer collapse when every provider header is filled in

<!-- lang:ru -->

### Добавлено

- Подписку можно забирать по зашифрованному каналу. В адресе нет ничего читаемого, ответ выглядит как любой другой, а каждый запрос дополняется до одного размера. По умолчанию выключено и включается для отдельной подписки; ключ прослойки закрепляется при первой встрече, и его отпечаток виден рядом с галочкой
- «Подключаться при запуске» — приложение может повторить нажатие кнопки сразу после старта. По умолчанию выключено

### Изменено

- В настройки системного прокси ничего не прописывается, пока не выбран способ подключения и не запрошено само подключение. Просто запущенное приложение — и тем более приложение без подписки — больше не настраивает систему само; удаление последней подписки снимает настройку сразу
- Экран настроек следует способу подключения, заданному провайдером: способа, которого в нём нет, на экране нет вовсе, а у названного написано, кто его выбрал. Лишний способ, оставшийся с прошлого раза, выключается, в том числе в настройках системы
- Окно расширенного режима по умолчанию шире, а содержимое прокручивается, а не сжимается: карточка «Сеть» и плитки больше не схлопываются, когда заполнены все заголовки провайдера

---

## v0.0.25-alpha

<!-- lang:en -->

### Added

- `clod-connect-mode` — the provider can say what the Connect button raises: the tunnel, the system proxy, or both. Your own choice still wins, unless the profile is locked
- `clod-device-remove` — a link to the page where a device slot is freed. The device-limit dialog now offers that instead of only pointing at support
- `clod-latency-style` — latency drawn as bars (the default), a coloured dot, or a plain number
- Errors from the core are explained in plain words. Sixteen common cases — domain not found, connection refused, port taken, access denied by the system, certificate trouble, a broken configuration, a panel answering 401/403/404/5xx — now come with a sentence telling you what to check; the original text stays next to it
- The lock a provider can put on the modes is no longer forever. It holds while the panel keeps confirming it and lifts on its own once the panel has been silent for days, and both the home screen and the settings now say who set the mode and how it goes away
- The app notices the machine waking up and the network changing: the system proxy is put back and stale connections are closed instead of waiting for the next thing to break
- A subscription is fetched by whichever route works — directly, through the app's own core, or through the system proxy — so a blocked subscription domain is still reachable through a tunnel that is already up

### Fixed

- A subscription takes its name from the panel the moment it is added, instead of appearing as a nameless placeholder until the first manual refresh
- Switching the interface mode resizes the window whoever switched it. A mode that arrived from the panel used to change the layout only, leaving the advanced interface scrolled out of sight in a narrow window
- The server drawer is alive while it is open: the list refreshes itself, and its header says how old the latency figures are
- Coming back from the tray refreshes the screen at once, instead of showing the previous session's numbers for up to a minute
- Groups that pick a server on their own (url-test, fallback, load balance) are left to their choice, instead of being pushed back onto the node saved from the last run
- The management interface gets its own secret per installation, instead of the one shipped in the template
- Configuration and profile files are written whole or not at all, so a crash mid-save cannot leave a half-written config behind
- A subscription is fetched over http and https only, and the app refuses to follow a subscription link back into itself
- Addresses and tokens are masked in the log by the logger itself, so no future log line can leak them by accident
- The core is only started if it is the binary the app already knows; a replaced file is refused with an explanation
- The DNS page owns the whole DNS block while its switch is on, and never leaves the tunnel without a resolver
- The provider logo is deleted along with the subscription, one device slot costs one permission prompt, "Test all" measures every node once instead of twice, and the subscription deadline is read correctly from panels that send it in milliseconds
- Two caches that grew for the whole session — measured latencies and the query mirror — now evict what they no longer need

### Changed

- Settings use one layout in both modes, and the simple mode has its way back into them again
- Sixteen known vulnerabilities in dependencies are closed, git dependencies are pinned to exact revisions, and the audit now runs weekly on its own
- The tray and the system notifications no longer say "Clash Verge" in any of the thirteen languages
- The provider documentation covers the new headers

<!-- lang:ru -->

### Добавлено

- `clod-connect-mode` — провайдер может сказать, что поднимает кнопка «Подключить»: туннель, системный прокси или оба. Ваш собственный выбор по-прежнему важнее, кроме запертого профиля
- `clod-device-remove` — ссылка на страницу, где освобождается слот устройства. Диалог лимита теперь предлагает её, а не только поддержку
- `clod-latency-style` — задержка полосками (по умолчанию), цветной точкой или числом
- Ошибки ядра объясняются словами. Шестнадцать частых случаев — домен не найден, соединение отклонено, порт занят, доступ запрещён системой, беда с сертификатом, битая конфигурация, ответ панели 401/403/404/5xx — теперь сопровождаются фразой о том, что проверять; исходный текст остаётся рядом
- Замок, которым провайдер запирает режимы, перестал быть вечным. Он держится, пока панель его подтверждает, и снимается сам, если панель молчит несколько дней; и главный экран, и настройки теперь говорят, кто задал режим и как это снимается
- Приложение замечает пробуждение машины и смену сети: системный прокси поднимается заново, а мёртвые соединения закрываются, вместо того чтобы ждать следующей поломки
- Подписка забирается тем путём, который сработает, — напрямую, через собственное ядро или через системный прокси, — поэтому заблокированный домен подписки достижим через уже поднятый туннель

### Исправлено

- Подписка берёт имя от панели сразу при добавлении, а не появляется безымянной болванкой до первого ручного обновления
- Смена режима интерфейса меняет и размер окна, кто бы её ни сделал. Режим, пришедший от панели, раньше менял одну вёрстку, и расширенный интерфейс оставался в узком окне, уехав в прокрутку
- Шторка серверов живая, пока открыта: список обновляется сам, а в её шапке видно, насколько свежие в нём задержки
- Возврат из трея обновляет экран сразу, а не показывает до минуты цифры прошлого показа
- Группы, которые выбирают сервер сами (url-test, fallback, балансировщик), больше не сбрасываются на узел, сохранённый с прошлого запуска
- Управляющий интерфейс получает свой секрет на каждую установку вместо общего из шаблона
- Файлы настроек и профилей пишутся целиком или никак: сбой посреди сохранения больше не оставляет обрезанный конфиг
- За подпиской приложение ходит только по http и https и отказывается заворачивать ссылку подписки на само себя
- Адреса и токены прячет сам логгер — значит, ни одна будущая строка лога не унесёт их случайно
- Ядро запускается, только если это тот бинарь, который приложение уже видело; подменённый файл получает отказ с объяснением
- Страница DNS владеет всем блоком DNS, пока её тумблер включён, и никогда не оставляет туннель без резолвера
- Логотип провайдера удаляется вместе с подпиской, установка службы стоит одного запроса прав, «Проверить все» меряет каждый узел один раз вместо двух, а срок подписки читается верно и у панелей, присылающих его в миллисекундах
- Два кэша, которые росли весь сеанс — измеренные задержки и зеркало запросов, — выбрасывают ненужное

### Изменено

- Настройки выглядят одинаково в обоих режимах, а из простого снова есть выход в них
- Закрыты шестнадцать известных уязвимостей в зависимостях, git-зависимости запинены по ревизиям, а проверка теперь идёт раз в неделю сама
- Трей и системные уведомления больше не говорят «Clash Verge» ни на одном из тринадцати языков
- Документация для провайдера описывает новые заголовки

---

## v0.0.24-alpha

<!-- lang:en -->

### Fixed

- The core is now watched in service mode too. Under the background service nobody was told when the core died: it was never restarted, no crash was reported, and the button stayed green while the traffic went nowhere — in the very mode TUN runs in. The app now asks the core for its version every half a minute and treats two silent rounds in a row as a failure
- After the core is restarted the screen refreshes itself, instead of showing the servers, groups and pings of the process that died
- The tray says what is, not what was asked for. The icon, the "TUN mode" checkmark and the tooltip used to read the saved setting, so a suppressed tunnel showed a checkmark in the tray at the same moment the home screen said it had not started
- An outdated background service is visible and repairable again. Such a service still answers, so the app counted TUN as available: it never offered to repair anything, and the toast that said "repair it in the settings" led to a page with no such button. The button is there now, and it picks the smallest action itself — start, install or repair
- Windows says why installing the service failed. Any refusal used to read as "Unknown error", including the most common one of all: a dismissed permission prompt
- A device over the limit no longer keeps using the servers it had saved. The panel answers a device limit by taking the servers away; the client used to keep the previous configuration, so the limit did not apply to that device at all
- The screen that explains an empty server list now names the device limit instead of blaming the provider
- The spare subscription address (`fallback-url`) is accepted over https only, like every other link the panel sends. It was the one exception, so a tampered response could move the subscription download to plain HTTP
- Two settings applied at once no longer undo each other. Pressing Connect while flipping a tray switch could roll back the other one's change: both went through a single draft, and a failure discarded all of it

### Added

- Settings in the simple mode are short again: connection, autostart, language, theme, device identification, the support report and the version. Everything else is not taken away but tucked under "Advanced settings" — a collapsible block of four groups
- A switch for pre-release builds. It is on while the project is in alpha, and one click makes the client wait for the first stable release instead
- `clod-show-0hosts` — a provider can turn off the client's own "no servers" screens and show the panel's placeholder nodes as they are, with its own wording
- `clod-hwid-limit` — the provider's own text for the device dialogs and cards
- The support report says how many nodes carry a description from the panel. Zero means the panel is not sending them, which is the usual answer to "why don't server descriptions show up"

### Changed

- The "Renew" and "Top up" buttons are gone, along with their headers. The only place the client points at for payment is the customer portal
- The sidebar is gone for good. It had not been drawn in any mode since the redesign, but the whole upstream menu machinery lived on behind it
- First tests on the frontend side: 26 of them, on the logic behind the empty-server-list reasons, the coloured markup in announcements and country detection

<!-- lang:ru -->

### Исправлено

- Ядро под службой теперь под присмотром. О его смерти не узнавал никто: не было ни перезапуска, ни сообщения о падении — кнопка оставалась зелёной, а трафик шёл мимо, причём именно в том режиме, в котором работает TUN. Приложение раз в полминуты спрашивает у ядра версию и считает отказом два молчания подряд
- После перезапуска ядра экран обновляется сам, а не показывает серверы, группы и задержки от умершего процесса
- Трей показывает то, что есть, а не то, что попросили. Значок, галочка «Режим TUN» и подсказка читали сохранённую настройку, поэтому при подавленном туннеле в трее стояла галочка ровно в тот момент, когда главный экран говорил «не запустился»
- Устаревшую службу снова видно, и её есть чем починить. Такая служба отвечает, поэтому TUN считался доступным: починку никто не предлагал, а тост «почините в настройках» вёл на страницу, где такой кнопки не было. Теперь она там есть и сама выбирает минимальное действие — запустить, поставить или починить
- Windows говорит, почему не удалось поставить службу. Раньше любой отказ читался как «Unknown error», включая самый частый — закрытый запрос прав
- Устройство сверх лимита больше не ходит по сохранённым серверам. Панель на лимит устройств отвечает тем, что забирает серверы; клиент оставлял прежнюю конфигурацию, и лимит на этом устройстве не работал вовсе
- Экран, объясняющий пустой список серверов, называет лимит устройств своим именем, а не винит провайдера
- Запасной адрес подписки (`fallback-url`) принимается только по https, как и все остальные ссылки от панели. Он был единственным исключением, и подменённый ответ мог увести загрузку подписки на обычный HTTP
- Две настройки, применённые одновременно, больше не отменяют друг друга. Нажатие Connect вместе с тумблером в трее могло откатить чужую правку: оба шли через один черновик, и провал сбрасывал его целиком

### Добавлено

- Настройки в простом режиме снова короткие: подключение, автозапуск, язык, тема, идентификация устройства, отчёт для поддержки и версия. Остальное не отобрано, а убрано под «Продвинутые настройки» — раскрывающийся блок из четырёх групп
- Тумблер предварительных сборок. Пока проект в альфе он включён, и одним щелчком клиент переводится в режим ожидания первого стабильного релиза
- `clod-show-0hosts` — провайдер может отключить наши экраны «нет серверов» и показать узлы-заглушки панели как есть, своими словами
- `clod-hwid-limit` — текст провайдера для диалогов и карточек устройства
- Отчёт для поддержки говорит, у скольких узлов есть описание от панели. Ноль означает, что панель их не присылает, — обычный ответ на вопрос «почему не появляются описания серверов»

### Изменено

- Кнопки «Продлить» и «Докупить» убраны вместе со своими заголовками. Единственная точка оплаты, на которую указывает клиент, — личный кабинет
- Боковая колонка удалена окончательно. Она не отрисовывалась ни в одном режиме с самого редизайна, но за ней жила вся апстримная машинерия меню
- Первые тесты на стороне фронта: 26 штук — на логику причин пустого списка серверов, цветовую разметку объявлений и определение страны по имени узла

---

## v0.0.23-alpha

<!-- lang:en -->

### Fixed

- The ping of the selected server no longer disappears while the subscription updates. The core rebuilds every connection when it rereads its configuration, and its measurements start empty — the number on screen turned into the word "Server" for a second or two, sometimes ten, and then came back. The row now keeps showing the last figure it had until a new one arrives
- The ping shown is the newest one there is. A measurement taken by hand on the Proxies page used to outrank anything the core measured later, for up to half an hour, and the automatic re-measurement that is supposed to catch a stale number was disabled by the very number it was guarding
- The selected server is no longer switched behind your back. The client used to pick a favourite node of its own accord whenever a measurement failed — on every launch and after every subscription update — and wrote that choice into the subscription, so a pinned server was lost for good. Leaving a dead node is the core's job, and it does it
- Server descriptions from the panel show up right after launch, instead of only after the configuration has been updated once
- TUN no longer stays green over a tunnel that is not there. The tunnel was confirmed once, three seconds after being switched on, and never again: a device that failed to come up a moment later, or a tunnel that died an hour in, left the button connected while the traffic went past it. It is now re-checked for as long as TUN is on
- An unanswered permission prompt no longer freezes the app. Waiting for it held everything else back — starting the core, restarting it, applying settings — until the app was killed. The prompt is no longer waited on forever, and answering it late still installs the service
- The subscription card is re-read when the window comes back from the tray, instead of showing the traffic and expiry of whenever it was last looked at

### Changed

- Memory no longer grows while the app sits in the tray: the provider's logo was cached anew on every subscription update and never released, a websocket that failed to close was left behind with nothing able to close it, and the log stream kept running in a hidden window

<!-- lang:ru -->

### Исправлено

- Пинг выбранного сервера больше не пропадает во время обновления подписки. Ядро пересобирает все соединения, когда перечитывает конфигурацию, и его замеры начинаются с нуля — цифра на экране превращалась в слово «Сервер» на секунду-другую, иногда на десять, и возвращалась. Теперь строка держит последнее значение, пока не приедет новое
- Показывается самый свежий замер, какой есть. Замер, снятый вручную на странице «Прокси», до получаса перебивал всё, что ядро померило позже, а автоматическая перепроверка, которая должна ловить протухшую цифру, отключалась ровно этой цифрой
- Выбранный сервер больше не переключается сам. Клиент по своей воле уходил на избранный узел при каждом неудачном замере — при запуске и после каждого обновления подписки — и записывал этот выбор в подписку, так что закреплённый сервер терялся навсегда. Уход с мёртвого узла — работа ядра, и оно её делает
- Описания серверов из панели появляются сразу после запуска, а не только после того, как конфигурация хоть раз обновилась
- TUN больше не горит зелёным над несуществующим туннелем. Туннель подтверждался один раз, через три секунды после включения, и больше никогда: устройство, не поднявшееся чуть позже, или туннель, умерший через час, оставляли кнопку подключённой, пока трафик шёл мимо. Теперь проверка идёт всё время, пока TUN включён
- Незакрытый запрос прав больше не вешает приложение. Ожидание держало всё остальное — запуск ядра, перезапуск, применение настроек — до тех пор, пока приложение не убьют. Ждать бесконечно мы перестали, а позднее подтверждение по-прежнему ставит службу
- Карточка подписки перечитывается, когда окно возвращается из трея, а не показывает трафик и срок на момент последнего взгляда

### Изменено

- Память больше не растёт, пока приложение лежит в трее: логотип провайдера кэшировался заново при каждом обновлении подписки и не освобождался, не закрывшийся веб-сокет оставался жить без возможности его закрыть, а поток логов продолжал работать в скрытом окне

---

## v0.0.22-alpha

<!-- lang:en -->

### Fixed

- The first press of Connect works again when TUN is on. With the tunnel up and the system proxy off the button was dark, but the press meant "disconnect": it switched TUN back off, and only the second press connected. What a press does now follows what the button shows
- Turning TUN on no longer reports success over a dead tunnel. Switching it on handed the core a configuration with no tunnel in it, because the session-wide suppression was lifted only after that configuration had already been built
- The TUN switch no longer turns itself off a few seconds after being turned on: the check that watches the core for a failed tunnel was reading complaints left over from an earlier attempt
- The state of TUN is re-read the moment the backend reports it, instead of up to ten seconds later - and at all in a window that was sent to the tray
- A configuration change the core refuses is no longer kept: the rejected draft used to stay behind and reach the config with the next successful change

<!-- lang:ru -->

### Исправлено

- Первое нажатие «Подключить» снова срабатывает при включённом TUN. Туннель поднят, системный прокси выключен — кнопка тёмная, но нажатие означало «отключить»: TUN гас, и подключало только второе нажатие. Теперь нажатие делает то, что кнопка показывает
- Включение TUN больше не рапортует об успехе над мёртвым туннелем. Ядру уходила конфигурация без туннеля: сессионное подавление снималось уже после того, как она была собрана
- Переключатель TUN больше не выключается сам через несколько секунд после включения: проверка, которая следит за жалобами ядра на туннель, читала жалобы от прошлой попытки
- Состояние TUN перечитывается сразу, как только бэкенд о нём сообщил, а не через десять секунд — и вообще перечитывается в окне, убранном в трей
- Настройка, которую ядро не приняло, больше не оседает в приложении: отклонённый черновик оставался и уезжал в конфигурацию со следующим удачным изменением

---

## v0.0.20-alpha

<!-- lang:en -->

### Fixed

- The home screen no longer shows yesterday's numbers. Nothing on it was ever refreshed: the shared core state was read once at start-up and then only when the backend announced something. A window pulled out of the tray painted the delays, the selected server and the Connect state of whenever it was last looked at
- The server's ping is chased until it appears. Right after connecting, the core rereads its configuration and its delay history is empty — and only url-test groups are measured by the core itself, so a pinned server was left with no number at all
- TUN no longer lies. The Connect button and the switch in the settings read whether the tunnel is actually up, not what the configuration wishes for: a tunnel the core could not raise is suppressed by the backend, and both used to stay green over it
- Pressing Connect while connected says "Disconnecting…" instead of "Connecting…"
- The Connect button reads the real system proxy in the OS and rechecks it while the window is open — it goes out from under us when another VPN client, or a dead core, takes it away

### Changed

- The hours left on a subscription are marked as rounded: "~5 h". They always were rounded up; now it says so

<!-- lang:ru -->

### Исправлено

- Главный экран больше не показывает вчерашние цифры. Он не обновлялся вообще: общее состояние ядра читалось один раз при запуске и дальше — только когда бэкенд сам о чём-то сообщал. Окно, поднятое из трея, рисовало задержки, выбранный сервер и состояние подключения такими, какими их видели в прошлый раз
- Пинг сервера теперь добивается до появления. Сразу после подключения ядро перечитывает конфигурацию, и история задержек в нём пуста, — а само ядро проверяет только url-test группы, поэтому у закреплённого сервера цифры не было вовсе
- TUN больше не врёт. Кнопка подключения и переключатель в настройках показывают, поднят ли туннель на самом деле, а не то, чего хочет конфигурация: туннель, который ядро не смогло поднять, бэкенд подавляет — и оба оставались зелёными над ним
- Нажатие на кнопку при активном подключении подписывается «Отключение…», а не «Подключение…»
- Кнопка подключения читает настоящий системный прокси в системе и перечитывает его, пока окно открыто: прокси у нас забирает и другой VPN-клиент, и упавшее ядро

### Изменено

- Часы до конца подписки помечены как округлённые: «~5 ч». Округление вверх было всегда, теперь об этом сказано

---

## v0.0.19-alpha

<!-- lang:en -->

### Fixed

- The endless "background service is outdated" is gone for good, at its actual root. The app shipped a service binary newer than the client library compiled into it, and the two could never agree — repairing reinstalled the very same mismatched version. They are now pinned to one version and only ever move together
- Repairing the service costs one elevation prompt instead of two: removal and installation run as a single privileged step on every platform
- An app update replaces the service properly: it is stopped before the files are swapped, so Windows can no longer revive the old binary mid-update
- The "service outdated" warning only appears while TUN is on. With TUN off nothing needs the service, and switching TUN on repairs it by itself

### Changed

- The service moved to its current release with the ownership model: the running core belongs to the app that started it, the service prepares the core's runtime itself, and subscription updates are applied in place — without restarting the core or dropping connections, now in service mode too

<!-- lang:ru -->

### Исправлено

- Вечная «фоновая служба устарела» убрана окончательно — по её настоящей причине. Приложение возило с собой службу новее, чем вкомпилированная в него клиентская библиотека, и они не могли договориться никогда; ремонт ставил ту же несовместимую версию. Теперь версии закреплены одной и меняются только вместе
- Ремонт службы стоит один запрос прав вместо двух: удаление и установка идут одним привилегированным шагом на всех платформах
- Обновление приложения подменяет службу правильно: перед заменой файлов она останавливается, и Windows больше не может воскресить старый бинарь посреди обновления
- Предупреждение «служба устарела» показывается только при включённом TUN. С выключенным служба никому не нужна, а включение TUN само её чинит

### Изменено

- Служба переведена на актуальный выпуск с моделью владельца: работающее ядро принадлежит запустившему его приложению, рантайм ядра готовит сама служба, а обновления подписки применяются на месте — без перезапуска ядра и разрыва соединений теперь и в service-режиме

---

## v0.0.18-alpha

<!-- lang:en -->

### Fixed

- The background service that TUN needs is no longer reinstalled on every launch. It was set up on each start even with TUN switched off, an app that opens before the service does read "no service at all", and the Windows service installer reports success even when it did nothing — so the elevation prompt came back again and again. The service exists for TUN alone: with TUN off it is left untouched, with TUN on the app waits for it to come up, and asks for rights at most once per version
- On Windows the service is now registered by the app installer itself, which already runs elevated. There is no separate prompt on install or on update, and an update replaces the service together with the app, so their versions can no longer drift apart
- When the app does have to act on the service, it asks the system first and does the smallest thing that helps: stopped means start, outdated means repair, absent means install. Success is judged by the service actually answering, not by the installer's exit code
- Dialogs can no longer grow a horizontal scrollbar. A dialog asking for a fixed width wider than the window pushed itself past the right edge, cutting off text and buttons — the update window was the visible case, but every dialog was exposed

### Changed

- In its last day the expiry tile names the day: "today until 20:33" instead of a bare "until 20:33"

<!-- lang:ru -->

### Исправлено

- Фоновая служба, которая нужна для TUN, больше не переустанавливается при каждом запуске. Раньше приложение настраивало её на каждом старте, даже с выключенным TUN; вдобавок оно стартует раньше службы и читало это как «службы нет», а установщик службы на Windows отвечает «готово», даже когда ничего не делал, — и запрос прав всплывал снова и снова. Служба нужна ровно для TUN: выключен — её не трогают вовсе, включён — приложение сначала ждёт, пока она поднимется, и просит права не чаще одного раза на версию
- На Windows службу теперь регистрирует сам установщик приложения — он и так запускается с правами администратора. Отдельного запроса нет ни при установке, ни при обновлении, а обновление подменяет службу вместе с приложением, так что их версии больше не расходятся
- Если приложению всё же приходится вмешаться, оно сначала спрашивает систему и делает минимальное: остановлена — запускает, устарела — чинит, нет вовсе — ставит. Успехом считается ответ самой службы, а не код возврата установщика
- В диалогах больше не может появиться горизонтальная прокрутка. Диалог, просивший ширину больше окна, выдавливал себя за правый край и резал текст и кнопки — заметнее всего это было в окне обновления, но подвержены были все

### Изменено

- В последние сутки плитка срока называет день: «сегодня до 20:33» вместо просто «до 20:33»

---

## v0.0.17-alpha

<!-- lang:en -->

### Changed

- The subscription deadline is counted on the panel's clock instead of your device's. Every answer from the panel already carries its own clock, so the app compares the two on each refresh and remembers the difference — a computer whose time is wrong no longer gets a wrong countdown or wrong reminders, and it keeps working offline. The warning next to the hours now appears only when there is something to warn about: the clocks disagree, or we have never seen the panel's
- Nothing is polled behind a window you cannot see. The server list asked the core for delays every three seconds and the chain every five while the app sat in the tray; both are silent now and refresh the moment the window comes back. Window visibility is resolved once for the whole app instead of eleven times over, and the countdown wakes up once an hour while it shows days

### Fixed

- The traffic widget of the old sidebar kept two live connections to the core and redrew a chart nobody could see — the column itself has been hidden since the redesign. It is gone, and with it the two settings that pointed at it
- The "automatic delay detection" switch has been removed: it saved a value nothing has read since the redesign
- Expiry reminders no longer fall silent after a clock correction. A device running fast marked "7 / 3 / 1 day left" as already told, and once the real time arrived the reminders never came

<!-- lang:ru -->

### Изменено

- Срок подписки считается по часам панели, а не вашего устройства. Каждый ответ панели и так несёт её собственные часы, поэтому приложение сверяет их при каждом обновлении и запоминает разницу — на компьютере с неверным временем срок и напоминания больше не врут, и работает это офлайн. Предупреждение рядом с часами теперь появляется, только когда есть о чём предупреждать: часы разошлись или часов панели мы ещё не видели
- За окном, которого не видно, ничего не опрашивается. Список серверов дёргал ядро за задержками каждые три секунды, а цепочка — каждые пять, пока приложение лежало в трее; теперь оба молчат и обновляются в момент возврата окна. Видимость окна вычисляется один раз на всё приложение вместо одиннадцати, а отсчёт срока просыпается раз в час, пока показывает дни

### Исправлено

- Виджет трафика старой боковой колонки держал два живых соединения с ядром и перерисовывал график, которого никто не видел, — сама колонка спрятана ещё с редизайна. Он удалён, вместе с двумя настройками, которые им управляли
- Убран переключатель «автоопределение задержек»: он сохранял значение, которое с редизайна никто не читал
- Напоминания о сроке больше не замолкают после поправки часов. Устройство со спешащими часами помечало «осталось 7 / 3 / 1 день» как рассказанные, и когда приходило настоящее время, напоминания уже не приезжали

---

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
