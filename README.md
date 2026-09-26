<h1 align="center">
  Clod Clash
</h1>

<p align="center">
  Клиент для подписок Remnawave на Windows, macOS и Linux.
  <br>
  Ядро — <a href="https://github.com/MetaCubeX/mihomo">Mihomo</a>; вторым встроенным ядром идёт
  <a href="https://github.com/Mrvibecodic/clod-core">Clod Core</a> — наш форк Mihomo с патчами.
  <br>
  Форк <a href="https://github.com/clash-verge-rev/clash-verge-rev">Clash Verge Rev</a>.
</p>

<p align="center">
  Languages: <b>Русский</b> · <a href="./docs/README_en.md">English</a>
  ·
  <a href="https://mrvibecodic.github.io/clod-clash/">Документация</a>
  ·
  <a href="https://mrvibecodic.github.io/clod-clash/download">Скачать</a>
</p>

<p align="center">
  <a href="https://t.me/+2lmP1yhxpCE3MDcy">
    <img alt="Telegram-группа — новости и релизы" src="https://img.shields.io/badge/Telegram-%D0%93%D1%80%D1%83%D0%BF%D0%BF%D0%B0%20%E2%80%94%20%D0%BD%D0%BE%D0%B2%D0%BE%D1%81%D1%82%D0%B8-2AABEE?style=for-the-badge&logo=telegram&logoColor=white">
  </a>
  &nbsp;
  <a href="https://t.me/+8BJQXYXYLqM4YWYy">
    <img alt="Telegram-чат — помощь и обсуждение" src="https://img.shields.io/badge/Telegram-%D0%A7%D0%B0%D1%82%20%E2%80%94%20%D0%BF%D0%BE%D0%BC%D0%BE%D1%89%D1%8C-229ED9?style=for-the-badge&logo=telegram&logoColor=white">
  </a>
</p>

<p align="center">
  <b><a href="https://t.me/+2lmP1yhxpCE3MDcy">Группа</a></b> — новости и релизы
  · <b><a href="https://t.me/+8BJQXYXYLqM4YWYy">Чат</a></b> — вопросы, помощь и обсуждение
</p>

<p align="center">
  <img src="./website/public/screenshots/og.png" alt="Clod Clash" width="800">
</p>

---

## Что это

Вставляете ссылку на подписку, нажимаете одну кнопку — и компьютер подключён. Всё, что панель
сообщает о подписке (тариф, срок, трафик, логотип, объявления, ссылки на кабинет и поддержку),
приложение показывает на главном экране.

Техническая часть Clash Verge Rev — правила, соединения, логи, редакторы конфигов — никуда не
делась: она в расширенном режиме и в продвинутых настройках.

<p align="center">
  <img src="./website/public/screenshots/01-home-simple.png" alt="Простой режим" width="260">
  <img src="./website/public/screenshots/03-servers.png" alt="Выбор сервера" width="260">
  <img src="./website/public/screenshots/07-home-dark.png" alt="Тёмная тема" width="260">
</p>

## Что умеет

* **Одна кнопка.** Простой режим по умолчанию: подключение, сервер, трафик и срок. Расширенный
  режим включается в Настройки → Основные.
* **Системный прокси или TUN.** TUN перехватывает трафик всех программ. На Windows фоновую службу
  для него ставит установщик, на Linux — пакет, на macOS — само приложение при первом включении.
* **Подписка грузится, даже если домен провайдера заблокирован.** Сначала как задано в свойствах
  подписки в клиенте (по умолчанию напрямую), потом через уже поднятый туннель, потом через системный прокси. Понимает запасные адреса и переезд
  подписки на новый адрес.
* **Понятные состояния.** «Подписка истекла», «Трафик закончился», «Превышен лимит устройств»,
  «Провайдер не выдал серверы» — словами, а не пустым списком серверов.
* **Срок по часам панели.** Неверное время на компьютере срок не сдвигает. Когда срок истёк,
  подписка сама обновляется один раз, чтобы сразу показать продление или состояние «истекла».
* **Выбор сервера не сбрасывается** тестом задержек, обновлением подписки и перезапуском.
  Избранные серверы — наверху списка.
* **Неудачное обновление не ломает рабочий профиль.** Конфиг, который не принял ядро, не
  применяется, прежний остаётся. Исключение — отказ панели по лимиту устройств: тогда прежние
  серверы убираются, иначе лимит бы не работал.
* **Лимит устройств.** Идентификатор устройства стабилен между обновлениями; сам машинный
  идентификатор и имя компьютера никуда не уходят. Отправку можно выключить.
* **Отчёт для поддержки** одной кнопкой — без адресов подписки, токенов и истории посещений.
* **Два ядра.** Mihomo от MetaCubeX без изменений (по умолчанию) и
  [Clod Core](https://github.com/Mrvibecodic/clod-core) — наш форк на свежей ветке Alpha: новая
  библиотека TLS-отпечатков utls v1.9.0-mod-meta (Firefox 148, Safari 26.3); узел, у которого
  проверка зависла или оборвалась, проверяется второй раз и мёртвым считается, только если не
  ответил и тогда, а нестабильный узел проигрывает стабильному. Переключаются в Настройки → Продвинутые настройки → Настройки Clash → «Ядро
  Clash»; каждое ядро обновляется из своих релизов.
* **Обновляется само.** Галка «Предварительные сборки» в Настройки → Основные включает альфа- и
  бета-версии; без неё приходят только стабильные.
* **Не решает за пользователя.** Свои настройки пользователя важнее подсказок панели; чужой
  системный прокси не трогается; пустая группа серверов отвергает трафик, а не пускает его мимо
  туннеля.

## Установка

Все сборки — на [странице загрузки](https://mrvibecodic.github.io/clod-clash/download).

| Система | Что скачать |
| --- | --- |
| Windows x64 | установщик `.exe` — ставит приложение, службу для TUN и правило брандмауэра для ядер. Есть портативный `.zip`: работает без установки, но обновления в фоне не скачивает |
| macOS 11+ (Apple Silicon и Intel) | `.dmg`. Сборка без сертификата Apple: первый запуск разрешается в «Системные настройки → Конфиденциальность и безопасность». Или одной командой в «Терминале»: `curl -fsSL https://mrvibecodic.github.io/clod-clash/install-macos.sh \| bash` |
| Linux x86_64 | `.deb` или `.rpm`. На системах с systemd пакет сразу регистрирует службу для TUN |

Подробно — в разделе [«Установка»](https://mrvibecodic.github.io/clod-clash/docs/install).

## Провайдерам

**Обязательно:** в панели в Subscription response rules добавьте правило для `User-Agent`
по регулярке `^ClodClash` с форматом ответа **MIHOMO**. Без него панель отдаст ответ по
умолчанию, и клиент подписку не примет. Всё про панель и шаблон — в
[docs/REMNAWAVE.md](./docs/REMNAWAVE.md).

**Заголовки `clod-*`** — наши собственные заголовки ответа. Панель их сама не шлёт: их
вписывают в `customResponseHeaders`. Каждый действует только в своей подписке.

| Заголовок | Значение | Что делает | Где работает |
| --- | --- | --- | --- |
| `clod-portal-url` | ссылка `https://` | кнопка «Кабинет» — личный кабинет, где продлевают тариф | ПК и Android |
| `clod-bot-url` | ссылка `https://`, `tg:` или `mailto:` | кнопка «Бот» | ПК и Android |
| `clod-monitor-url` | ссылка `https://` | кнопка «Мониторинг» — состояние серверов | ПК и Android |
| `clod-guide-url` | ссылка `https://` | кнопка «Инструкция» | ПК и Android |
| `clod-announce` | текст | постоянное объявление; если пришёл и `announce`, показывается `clod-announce` | ПК и Android |
| `clod-promo` | текст | промо-баннер, который можно закрыть | ПК и Android |
| `clod-promo-url` | ссылка `https://` | куда ведёт нажатие на промо | ПК и Android |
| `clod-lock-mode` | `true` / `lock` / `false` | запирает режим маршрутизации на `mode` из шаблона: `true` — пока панель это подтверждает, `lock` — навсегда, `false` — не запирать | ПК и Android |
| `clod-ping` | `A/B` в мс, например `150/300` | границы цвета пинга: до `A` зелёный, до `B` жёлтый, дальше красный. Без заголовка — `200/400` | ПК и Android |
| `clod-disable-ping` | только `true` | галочка или крестик вместо миллисекунд | ПК и Android |
| `clod-show-0hosts` | `true` / `false` | показывать узлы-заглушки панели как есть вместо экрана клиента с причиной | ПК и Android |
| `clod-connect-mode` | `tun` / `proxy` / `both` | что включает кнопка подключения. Без заголовка — системный прокси | только ПК |
| `clod-simple-mode` | `true` / `false` | простой или расширенный вид по умолчанию. Без заголовка — простой | только ПК |
| `clod-latency-style` | `bars` / `dot` / `number` | как рисовать пинг выбранного сервера: полоски, точка или число. Без заголовка — полоски | только ПК |
| `clod-theme` | `accent=#RRGGBB; mode=light\|dark; background=https://…` | акцентный цвет, светлая или тёмная тема и фон окна | только ПК |

Вместо `true` / `false` подходят `1` / `0`, `yes` / `no`, `on` / `off` (кроме
`clod-disable-ping`). Текст с кириллицей отдавайте как `base64:<текст в base64>`. Непонятое
значение клиент считает отсутствием заголовка.
Подробно о каждом заголовке и полный список стандартных — в [docs/HEADERS.md](./docs/HEADERS.md).

## Документация

* [Сайт с документацией](https://mrvibecodic.github.io/clod-clash/) — установка, первое
  подключение, настройки, «если не работает».
* [docs/HEADERS.md](./docs/HEADERS.md) — все заголовки подписки: что клиент шлёт и что понимает.
* [docs/REMNAWAVE.md](./docs/REMNAWAVE.md) — настройка панели и шаблона MIHOMO.
* [docs/BUILDING.md](./docs/BUILDING.md) — сборка из исходников.
* [docs/RELEASING.md](./docs/RELEASING.md) — выпуск релиза.

## Сборка

Коротко: `pnpm install`, `pnpm prebuild`, `pnpm dev`. Подробно — в [docs/BUILDING.md](./docs/BUILDING.md).

## Благодарности

Clod Clash не существовал бы без этих проектов:

* [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo) — ядро, на котором всё работает.
  По умолчанию используется официальный бинарник без изменений; второе встроенное ядро,
  [Clod Core](https://github.com/Mrvibecodic/clod-core), — наш форк Mihomo с патчами.
* [clash-verge-rev/clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev) —
  приложение, форком которого является Clod Clash. Весь интерфейс, работа с профилями,
  системный прокси, TUN, служба, трей — их работа.
* [zzzgydi/clash-verge](https://github.com/zzzgydi/clash-verge) — оригинальный Clash Verge,
  с которого началась эта линия клиентов.
* [tauri-apps/tauri](https://github.com/tauri-apps/tauri) — фреймворк приложения.
* [Dreamacro/clash](https://github.com/Dreamacro/clash) — прародитель ядра.
* [remnawave/panel](https://github.com/remnawave/panel) — панель, под которую сделан форк;
  базовый набор заголовков подписки взят из её реализации.

Компоненты, которые едут вместе с приложением:

* [clash-verge-rev/clash-verge-service-ipc](https://github.com/clash-verge-rev/clash-verge-service-ipc) —
  системная служба, без которой не работает TUN, и протокол общения с ней.
* [clash-verge-rev/sysproxy-rs](https://github.com/clash-verge-rev/sysproxy-rs),
  [tauri-plugin-mihomo](https://github.com/clash-verge-rev/tauri-plugin-mihomo),
  [clash-verge-logger](https://github.com/clash-verge-rev/clash-verge-logger) — системный прокси,
  клиент к API ядра и логгер.
* [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat) — базы `country.mmdb`,
  `geosite.dat`, `geoip.dat` в поставке.
* [Kuingsmile/uwp-tool](https://github.com/Kuingsmile/uwp-tool) — `enableLoopback.exe` для
  UWP-приложений на Windows; NSIS Simple Service Plugin — в установщике.
* Интерфейс построен на [React](https://react.dev), [MUI](https://mui.com),
  [Emotion](https://emotion.sh), [i18next](https://www.i18next.com),
  [Monaco Editor](https://microsoft.github.io/monaco-editor/), SWR, ahooks и dnd-kit.

Отдельно — проектам, у которых мы подсмотрели продуктовые решения и состав заголовков.
Чужого кода в них мы не брали, только форматы и подходы:
[FlClash](https://github.com/chen08209/FlClash) и его форк
[FlClashX](https://github.com/pluralplay/FlClashX) — синоним заголовка режима интерфейса и схема
«желаемое против фактического» для TUN; [koala-clash](https://github.com/coolcoala/koala-clash) —
формат User-Agent и подача версии системы вместо имени компьютера;
[Prizrak-Box](https://github.com/legiz-ru/Prizrak-Box) — синтаксис цветовой разметки `#RRGGBB`
в объявлениях, синоним `global-mode` и описание сервера из подписки;
[dropweb](https://github.com/enkinvsh/dropweb) — защиты кэша логотипа, редакция адресов в логах
и отчёт для поддержки.

Флаги стран в селекторе серверов — набор
[HatScripts/circle-flags](https://github.com/HatScripts/circle-flags) (MIT). Он поставляется
с приложением локально (`src/public/flags`, там же текст лицензии): эмодзи-флаги не рисуются
на Windows, а тянуть их из сети клиенту, работающему во враждебной сети, нельзя.

## Лицензия

GPL-3.0, как и у Clash Verge Rev. Текст — в файле [LICENSE](./LICENSE).

Clod Clash — изменённая версия [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev)
`v2.5.2`. Изменения внесены в 2026 году; в исходниках они помечены маркером `clod:` — по нему видно,
что именно отличается от апстрима.

Флаги стран в `src/public/flags` — набор
[HatScripts/circle-flags](https://github.com/HatScripts/circle-flags), лицензия MIT, текст лежит
рядом с ними в [`src/public/flags/LICENSE`](./src/public/flags/LICENSE). Шрифт
`src/assets/fonts/Twemoji.Mozilla.ttf` — сборка Mozilla на основе
[Twemoji](https://github.com/jdecked/twemoji); графика Twemoji распространяется по
[CC-BY 4.0](https://creativecommons.org/licenses/by/4.0/), авторство — Twitter, Inc. и участники
проекта.
