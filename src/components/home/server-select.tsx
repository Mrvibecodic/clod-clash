import AltRouteRoundedIcon from '@mui/icons-material/AltRouteRounded'
import BoltRoundedIcon from '@mui/icons-material/BoltRounded'
import CheckRoundedIcon from '@mui/icons-material/CheckRounded'
import ExpandMoreRoundedIcon from '@mui/icons-material/ExpandMoreRounded'
import StarBorderRoundedIcon from '@mui/icons-material/StarBorderRounded'
import StarRoundedIcon from '@mui/icons-material/StarRounded'
import {
  alpha,
  Box,
  Button,
  ButtonBase,
  CircularProgress,
  Drawer,
  IconButton,
  List,
  ListItemButton,
  Stack,
  Typography,
} from '@mui/material'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CountryFlag } from '@/components/home/country-flag'
import { NoServersStatus } from '@/components/home/no-servers-status'
import { useDrawerCapHeight } from '@/hooks/use-drawer-cap-height'
import { useGroupDelayTest } from '@/hooks/use-group-delay-test'
import { useGroupTestUrls } from '@/hooks/use-group-test-urls'
import { useNoServersStatus } from '@/hooks/use-no-servers-status'
import { useProfiles } from '@/hooks/use-profiles'
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useServerDescriptions } from '@/hooks/use-server-descriptions'
import { useVisibility } from '@/hooks/use-visibility'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import delayManager from '@/services/delay'
import { showNotice } from '@/services/notice-service'
import { nameWithoutFlag } from '@/utils/country'
import { delayColor } from '@/utils/delay-color'
import {
  AUTO_GROUP_TYPES,
  displayLeaf,
  entryDelay,
  entryMeasuredAt,
  entryPingTarget,
  groupType,
  hasRealNodes,
  isCorePlaceholder,
  NON_NODE_TYPES,
  type ProxyNode,
  SELECTABLE_GROUP_TYPES,
  usableDelay,
  visibleGroups,
} from '@/utils/proxy-groups'
import { toUnixSeconds } from '@/utils/subscription-status'

const VIRTUALIZE_FROM = 50
const ROW_HEIGHT = 52

/**
 * clod:drawer-freshness — как часто перечитывать список, пока шторка открыта.
 *
 * Пять секунд — это один дешёвый запрос к уже поднятому ядру: столько же
 * стоит опрос главного экрана, а шторку держат открытой секунды, не часы.
 */
const DRAWER_REFRESH_MS = 5000

/** Как часто пересчитывается возраст показанных задержек. */
const AGE_TICK_MS = 5000

interface Props {
  open: boolean
  onClose: () => void
}

/**
 * Full server list, opened over the home screen rather than as its own page:
 * picking a server is a detour from connecting, not a destination.
 *
 * Groups sit in a scrollable row of chips above the list — templates in the
 * wild (Davoyan, legiz) ship half a dozen per-service groups plus balancers,
 * and a dropdown hid all of that.
 */
export const ServerSelect = ({ open, onClose }: Props) => {
  const { t } = useTranslation()
  const { proxies } = useProxiesData()
  const { refreshProxy } = useAppRefreshers()
  // `selectNodeForGroup` talks straight to the core, so nothing tells the
  // frontend to re-read the proxies — without the explicit refresh the tick
  // and the "current server" row keep showing the previous node.
  const { changeProxy } = useProxySelection({
    onSuccess: () => {
      refreshProxy().catch(() => {})
    },
    onError: (error) => showNotice.error(error),
  })
  const [testing, setTesting] = useState(false)
  const scrollRef = useRef<HTMLDivElement>(null)
  // clod: подтягивает URL тестов групп из шаблона, чтобы показанные задержки
  // соответствовали тому, чем группу реально мерили
  useGroupTestUrls()

  const records = (proxies?.records ?? {}) as Record<string, any>
  // clod: описания серверов приходят не от ядра, а из самой подписки — пустая
  // карта здесь норма, строки просто остаются такими, как были
  const descriptions = useServerDescriptions()
  const groups = useMemo(() => visibleGroups(proxies), [proxies])
  const [groupName, setGroupName] = useState<string>('')
  const group = useMemo(
    () => groups.find((item) => item.name === groupName) ?? groups[0],
    [groups, groupName],
  )
  const canSelect = SELECTABLE_GROUP_TYPES.has(groupType(group))

  // Starred servers float to the top of the list; the stars live on the
  // profile, so they survive subscription updates and app restarts.
  const { current, patchCurrent, mutateProfiles } = useProfiles()
  const favorites = useMemo(
    () => new Set(current?.favorites ?? []),
    [current?.favorites],
  )

  const toggleFavorite = useLockFn(async (nodeName: string) => {
    if (!current?.uid) return
    const stored = current.favorites ?? []
    const next = favorites.has(nodeName)
      ? stored.filter((name) => name !== nodeName)
      : [...stored, nodeName]
    try {
      await patchCurrent({ favorites: next })
    } catch (error) {
      showNotice.error(error)
    }
  })

  // Тот же вопрос, что и в строке на главной, и ровно тем же способом: есть ли
  // во всём конфиге хоть один настоящий сервер. Пока `proxies` не загружены
  // (старт приложения, рестарт ядра) — молчим: пустота ещё ничего не значит.
  const { show: noServers, onlySentinels } = useNoServersStatus(current)
  const listEmpty = Boolean(proxies) && !hasRealNodes(proxies)
  const showStatus = noServers && (onlySentinels || listEmpty)

  const nodes = useMemo(() => {
    // clod: core placeholders (`REJECT`…) are what a group is left with once
    // the sentinel filter dropped the panel's "subscription expired" stubs —
    // showing them as servers would recreate exactly the problem it solves.
    const all = (group?.all ?? []).filter(
      (node) => !isCorePlaceholder(node.name),
    )
    if (favorites.size === 0) return all
    return [
      ...all.filter((node) => favorites.has(node.name)),
      ...all.filter((node) => !favorites.has(node.name)),
    ]
  }, [group, favorites])

  const virtualizer = useVirtualizer({
    count: nodes.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
    enabled: nodes.length > VIRTUALIZE_FROM,
  })

  // clod:drawer-freshness — пока шторка открыта, список живой.
  //
  // Раньше он был снимком того, что лежало в кэше на момент открытия: узел
  // переключили из трея, ядро само ушло с мёртвого сервера, тест в другом окне
  // домерил задержки — на экране всё это появлялось только после закрытия и
  // повторного открытия. Опрашиваем, лишь пока шторка ОТКРЫТА и окно видно:
  // за закрытой шторкой обновлять нечего, и это ровно то правило, по которому
  // живут остальные опросы.
  const visible = useVisibility()
  useEffect(() => {
    if (!open || !visible) return
    const timer = window.setInterval(() => {
      refreshProxy().catch(() => {})
    }, DRAWER_REFRESH_MS)
    return () => window.clearInterval(timer)
  }, [open, visible, refreshProxy])

  // Возраст самого свежего замера в группе: одна честная строка вместо
  // подписи под каждой строкой (у каждого узла свой возраст, но пользователю
  // важно «эти цифры вообще сегодняшние или нет»).
  // Часы, а не счётчик тиков: возраст считается ОТ этого значения, поэтому
  // подпись стареет сама, даже когда список не меняется.
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!open || !visible) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), AGE_TICK_MS)
    return () => window.clearInterval(timer)
  }, [open, visible])

  const measuredLabel = useMemo(() => {
    if (!open || !group || nodes.length === 0) return undefined
    const newest = nodes.reduce((latest, node) => {
      const at = entryMeasuredAt(records, node.name, group.name)
      return at > latest ? at : latest
    }, 0)
    if (!newest) return undefined
    const minutes = Math.floor((now - newest) / 60_000)
    return minutes < 1
      ? t('home.components.serverSelect.measuredJustNow')
      : // ГРАБЛЯ i18next: число подставлять как `minutes`, НЕ `count` —
        // `count` считается плюральным, и i18next пошёл бы искать `_one`
        // и `_other`, которых наш генератор не делает.
        t('home.components.serverSelect.measuredMinutesAgo', { minutes })
  }, [open, group, nodes, records, now, t])

  const select = useLockFn(async (nodeName: string) => {
    if (!group || !canSelect) return
    await changeProxy(group.name, nodeName)
    onClose()
  })

  // clod: тест с keepFixed + восстановлением сохранённого выбора — иначе
  // mihomo сбрасывал закреплённый узел url-test групп при каждом тесте.
  const runGroupDelayTest = useGroupDelayTest()
  const runDelayTest = useCallback(async () => {
    if (!group) return
    setTesting(true)
    try {
      await runGroupDelayTest(group.name)
    } catch (error) {
      showNotice.error(error)
    } finally {
      setTesting(false)
    }
  }, [group, runGroupDelayTest])

  const typeLabel = (type: string) => {
    switch (type) {
      case 'urltest':
        return t('home.components.serverSelect.types.urltest')
      case 'fallback':
        return t('home.components.serverSelect.types.fallback')
      case 'loadbalance':
        return t('home.components.serverSelect.types.loadbalance')
      case 'smart':
        return t('home.components.serverSelect.types.smart')
      case 'selector':
        return t('home.components.serverSelect.types.selector')
      default:
        return type
    }
  }

  const renderRow = (node: ProxyNode) => {
    const record = records[node.name]
    const type = groupType(record ?? node)
    const isGroup = Boolean(record?.all) || NON_NODE_TYPES.has(type)
    const delay = entryDelay(records, node.name, group?.name ?? '')
    const selected = group?.now === node.name
    const starred = favorites.has(node.name)
    // clod: служебные имена ядра (COMPATIBLE и т.п.) в подписи не показываем
    const leaf = isGroup ? displayLeaf(records, node.name) : undefined
    // clod: слово провайдера о сервере вытесняет и тип узла, и «Используется»:
    // тип не говорил ничего, а выбор и без слов виден по галочке с подсветкой.
    // У групп описания нет — там подпись остаётся прежней.
    const description = isGroup ? undefined : descriptions[node.name]

    return (
      <ListItemButton
        key={node.name}
        selected={selected}
        onClick={() => void select(node.name)}
        sx={{
          borderRadius: 1,
          height: ROW_HEIGHT,
          gap: 1.25,
          cursor: canSelect ? 'pointer' : 'default',
        }}
      >
        {isGroup ? (
          <Box
            sx={{
              width: 26,
              height: 26,
              borderRadius: '50%',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flex: 'none',
              color: 'primary.main',
              bgcolor: (theme) => alpha(theme.palette.primary.main, 0.13),
            }}
          >
            {AUTO_GROUP_TYPES.has(type) ? (
              <BoltRoundedIcon sx={{ fontSize: 17 }} />
            ) : (
              <AltRouteRoundedIcon sx={{ fontSize: 16 }} />
            )}
          </Box>
        ) : (
          <CountryFlag name={node.name} />
        )}
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Typography noWrap>{nameWithoutFlag(node.name)}</Typography>
          {/* clod: описание от провайдера, иначе — «Используется» у активного
              сервера и тип узла у остальных */}
          <Typography
            variant="caption"
            color={selected && !description ? 'success.main' : 'text.secondary'}
            noWrap
            // 30 символов панель гарантирует, чужой шаблон — нет: полный текст
            // остаётся доступен наведением, даже если подпись его обрезала
            title={description}
          >
            {description ??
              (selected
                ? isGroup && leaf
                  ? `${t('home.components.serverSelect.inUse')} · ${nameWithoutFlag(leaf)}`
                  : t('home.components.serverSelect.inUse')
                : isGroup
                  ? leaf
                    ? `${typeLabel(type)} · ${nameWithoutFlag(leaf)}`
                    : typeLabel(type)
                  : node.type)}
          </Typography>
        </Box>
        {/* clod: маркер ошибки (1e6) — это не пинг, показываем прочерк */}
        <Typography
          variant="body2"
          sx={{
            color: usableDelay(delay) ? delayColor(delay) : 'text.disabled',
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          {usableDelay(delay) ? `${delay} ms` : '—'}
        </Typography>
        {isGroup ? null : (
          <IconButton
            size="small"
            aria-label={t('home.components.serverSelect.favorite')}
            sx={{ color: starred ? 'warning.main' : 'text.disabled' }}
            onClick={(event) => {
              event.stopPropagation()
              void toggleFavorite(node.name)
            }}
          >
            {starred ? (
              <StarRoundedIcon fontSize="small" />
            ) : (
              <StarBorderRoundedIcon fontSize="small" />
            )}
          </IconButton>
        )}
        {selected ? (
          <CheckRoundedIcon color="success" fontSize="small" />
        ) : null}
      </ListItemButton>
    )
  }

  // clod: the drawer grows with the group — three servers keep it low, twenty
  // push it up to the connect button and no further. The paper stays
  // content-sized (a bottom drawer does that on its own); only the ceiling
  // moves, so nothing here has to compute a height.
  const capHeight = useDrawerCapHeight(open)

  return (
    <Drawer
      anchor="bottom"
      open={open}
      onClose={onClose}
      slotProps={{
        paper: {
          sx: {
            maxHeight: capHeight,
            borderRadius: '12px 12px 0 0',
            // the paper scrolls on its own by default — with the list holding
            // the only scrollbar that would make a second, nested one
            overflow: 'hidden',
          },
        },
      }}
    >
      <Stack sx={{ p: 2, gap: 1.5, minHeight: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Typography variant="h6">
            {t('home.components.serverSelect.title')}
          </Typography>

          {/* clod:drawer-freshness — сколько лет цифрам в списке. Задержки
              приходят из истории замеров ядра, и без этой строки список
              одинаково уверенно показывал и секундной свежести замер, и
              позавчерашний: «26 ms» выглядит фактом, пока не сказано, когда
              его мерили. */}
          {measuredLabel ? (
            <Typography variant="caption" color="text.secondary" noWrap>
              {measuredLabel}
            </Typography>
          ) : null}

          <Box sx={{ flex: 1 }} />

          <Button
            size="small"
            startIcon={
              testing ? <CircularProgress size={16} /> : <BoltRoundedIcon />
            }
            disabled={testing || !group}
            onClick={() => void runDelayTest()}
          >
            {t('home.components.serverSelect.test')}
          </Button>
        </Stack>

        {/* Group chips in a row: every visible group of the template, side
            by side — never a dropdown. Scrolls when they don't fit. */}
        {groups.length > 1 ? (
          <Stack
            direction="row"
            sx={{
              gap: 0.75,
              overflowX: 'auto',
              flex: 'none',
              pb: 0.5,
              // a slim scrollbar so a long chip row stays discoverable
              '&::-webkit-scrollbar': { height: 4 },
              '&::-webkit-scrollbar-thumb': {
                bgcolor: 'divider',
                borderRadius: 2,
              },
            }}
          >
            {groups.map((item) => {
              const active = item.name === (group?.name ?? '')
              const auto = AUTO_GROUP_TYPES.has(groupType(item))
              // clod: the resolved node right on the chip, so checking what
              // each group runs on does not require opening every group
              const leaf = displayLeaf(records, item.name)
              return (
                <ButtonBase
                  key={item.name}
                  onClick={() => setGroupName(item.name)}
                  sx={{
                    px: 1.5,
                    py: 0.75,
                    borderRadius: '10px',
                    whiteSpace: 'nowrap',
                    flex: 'none',
                    flexDirection: 'column',
                    alignItems: 'flex-start',
                    justifyContent: 'center',
                    gap: 0.25,
                    color: active ? 'primary.main' : 'text.primary',
                    bgcolor: (theme) =>
                      active
                        ? alpha(theme.palette.primary.main, 0.13)
                        : theme.palette.action.hover,
                    border: (theme) =>
                      `1px solid ${
                        active ? theme.palette.primary.main : 'transparent'
                      }`,
                  }}
                >
                  <Box
                    sx={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 0.5,
                      fontSize: 13,
                      fontWeight: 600,
                    }}
                  >
                    {auto ? <BoltRoundedIcon sx={{ fontSize: 15 }} /> : null}
                    {nameWithoutFlag(item.name)}
                  </Box>
                  {leaf ? (
                    <Box
                      sx={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 0.5,
                        minWidth: 0,
                      }}
                    >
                      {/* clod: тот же флаг, что и в строках списка */}
                      <CountryFlag name={leaf} size={13} />
                      <Typography
                        variant="caption"
                        noWrap
                        sx={{
                          maxWidth: 180,
                          color: active ? 'primary.main' : 'text.secondary',
                          opacity: active ? 0.8 : 1,
                          lineHeight: 1.2,
                        }}
                      >
                        {nameWithoutFlag(leaf)}
                      </Typography>
                    </Box>
                  ) : null}
                </ButtonBase>
              )
            })}
          </Stack>
        ) : null}

        {!canSelect && group ? (
          <Typography variant="caption" color="text.secondary">
            {t('home.components.serverSelect.autoHint')}
          </Typography>
        ) : null}

        {/* clod: пустой список честен, но ничего не объясняет — статус говорит,
            почему серверов нет, и даёт ссылки провайдера. Показывается и когда
            в группе остался один `DIRECT`: подключаться всё равно не к чему. */}
        {showStatus ? (
          <NoServersStatus profile={current} onRefreshed={mutateProfiles} />
        ) : null}

        {nodes.length === 0 ? (
          <Typography
            color="text.secondary"
            sx={{ py: 4, textAlign: 'center' }}
          >
            {t('home.components.serverSelect.empty')}
          </Typography>
        ) : nodes.length > VIRTUALIZE_FROM ? (
          <Box ref={scrollRef} sx={{ overflowY: 'auto', minHeight: 0 }}>
            <Box
              sx={{
                height: virtualizer.getTotalSize(),
                position: 'relative',
                width: '100%',
              }}
            >
              {virtualizer.getVirtualItems().map((row) => (
                <Box
                  key={row.key}
                  sx={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${row.start}px)`,
                  }}
                >
                  {renderRow(nodes[row.index])}
                </Box>
              ))}
            </Box>
          </Box>
        ) : (
          <List sx={{ overflowY: 'auto', minHeight: 0 }}>
            {nodes.map(renderRow)}
          </List>
        )}
      </Stack>
    </Drawer>
  )
}

interface RowProps {
  onOpen: () => void
}

/**
 * clod:latency-style — на сколько делений тянет задержка.
 *
 * Одна лестница на все виды показа: и полоски, и точка красятся по одному
 * порогу, иначе «зелёная точка» и «четыре полоски» разошлись бы при первой же
 * правке.
 */
const latencyLevel = (delay?: number) =>
  !usableDelay(delay)
    ? 0
    : delay < 120
      ? 4
      : delay < 250
        ? 3
        : delay < 500
          ? 2
          : 1

const latencyColor = (level: number) =>
  level === 0 ? 'divider' : level >= 3 ? 'success.main' : 'warning.main'

/**
 * clod:latency-style — точка вместо полосок (`clod-latency-style: dot`).
 *
 * Панели, настроенные под Happ и Prizrak-Box, привыкли показывать задержку
 * цветной точкой; провайдеру, у которого так нарисованы все инструкции, дешевле
 * попросить точку, чем переучивать клиентов.
 */
const SignalDot = ({ delay }: { delay?: number }) => (
  <Box
    sx={{
      width: 10,
      height: 10,
      flex: 'none',
      borderRadius: '50%',
      bgcolor: latencyColor(latencyLevel(delay)),
    }}
  />
)

/** clod:latency-style — задержка числом (`clod-latency-style: number`). */
const LatencyNumber = ({ delay }: { delay?: number }) => (
  <Typography
    sx={{ fontSize: 12, flex: 'none', fontVariantNumeric: 'tabular-nums' }}
    color={
      usableDelay(delay) ? latencyColor(latencyLevel(delay)) : 'text.disabled'
    }
  >
    {usableDelay(delay) ? `${delay} ms` : '—'}
  </Typography>
)

/** Mockup-style four-bar signal indicator, coloured by latency. */
const SignalBars = ({ delay }: { delay?: number }) => {
  const lit = latencyLevel(delay)
  const color = (index: number) => (index < lit ? latencyColor(lit) : 'divider')
  return (
    <Box
      sx={{
        display: 'flex',
        alignItems: 'flex-end',
        gap: '2px',
        height: 16,
        flex: 'none',
      }}
    >
      {[5, 8, 11, 15].map((height, index) => (
        <Box
          key={height}
          sx={{
            width: '3.5px',
            height,
            borderRadius: '2px',
            bgcolor: color(index),
          }}
        />
      ))}
    </Box>
  )
}

// clod: подписка обновилась → ядро перечитало конфиг → история задержек в
// ядре обнулилась и пинги на главной «пропадали». Автотест гоняем один раз
// на каждую пару (группа, момент обновления подписки) — module-scope, чтобы
// ремоунт экрана не пинговал повторно.
let lastAutoDelayKey = ''

// clod: последний пинг, который пользователь реально видел. Module-scope по
// той же причине, что и ключ выше: ремоунт экрана не должен стирать цифру,
// которая только что была на месте. Состоянием это держать нельзя — запись
// шла бы из эффекта, то есть лишний рендер на каждый замер.
let lastKnownPing: { key: string; delay: number } | undefined

/**
 * clod: пинг старше этого — на экране враньё, а не данные.
 *
 * Цифра часовой давности выглядит точно так же, как снятая секунду назад.
 * Окно, пролежавшее в трее, возвращается именно с такой: ядро само проверяет
 * только url-test группы, а закреплённый узел `select`-группы не трогает
 * никто. Поэтому, показывая экран, мы не просто перечитываем ядро, а
 * перемеряем — если показанному замеру больше минуты.
 */
const PING_MAX_AGE_MS = 60_000
/** Цифра на экране есть — чаще раза в минуту не перемеряем. */
const PING_GAP_MS = 60_000
/**
 * Цифры нет вовсе — пробуем часто, пока не появится.
 *
 * Это не тот же случай, что «пинг устарел»: пустое место на экране надо
 * закрыть, а причины пустоты временные — ядро ещё поднимается, конфиг только
 * что перечитан, замер не прошёл. И повторять приходится по таймеру:
 * неудачный замер данных не меняет, а значит сам по себе эффект не
 * перезапустится и второй попытки не будет никогда.
 */
const PING_RETRY_MS = 5_000
/**
 * Сколько раз подряд пробуем, пока цифры нет.
 *
 * Потолок нужен для случая, когда падает сам запрос (ядро перезапускается,
 * API недоступен): истории тогда не появляется, замок по возрасту замера не
 * срабатывает — и без счётчика это был бы вечный запрос раз в пять секунд.
 * Счётчик обнуляется, как только цифра появилась или сменился узел.
 */
const PING_RETRY_LIMIT = 6
const PING_TIMEOUT_MS = 10_000
let lastAutoPingAt = 0

/** The compact row on the home screen: current server, latency, one tap. */
export const ServerSelectRow = ({ onOpen }: RowProps) => {
  const { t } = useTranslation()
  const { proxies } = useProxiesData()
  const { current: currentProfile } = useProfiles()
  const runGroupDelayTest = useGroupDelayTest()
  const descriptions = useServerDescriptions()
  const visible = useVisibility()
  const { urlFor } = useGroupTestUrls()
  const { refreshProxy } = useAppRefreshers()

  const records = (proxies?.records ?? {}) as Record<string, any>
  const group = visibleGroups(proxies)[0]
  // clod: an emptied group resolves to `REJECT` — that means "nothing to
  // connect to", not a server called REJECT, so the row stays "not selected".
  const selection = group?.now
  const current = isCorePlaceholder(selection) ? undefined : selection
  // The selection may be a balancer; the ping (and the flag) belong to the
  // node the chain actually lands on. Core placeholders (COMPATIBLE…) are
  // hidden — the flag then falls back to the selection's own name.
  const leaf = current ? displayLeaf(records, current) : undefined
  const flagName = leaf ?? current
  const delay = current
    ? entryDelay(records, current, group?.name ?? '')
    : undefined
  const measuredAt = current
    ? entryMeasuredAt(records, current, group?.name ?? '')
    : 0
  // Меряем ровно ту запись, чья цифра на экране (то же правило, что и в
  // `entryDelay`): у балансировщика это лист, на который приземляется цепочка,
  // у обычного узла — он сам.
  const pingTarget = current
    ? entryPingTarget(records, current, group?.name ?? '')
    : undefined
  const hasPing = usableDelay(delay)
  const pingProvider = pingTarget
    ? (records[pingTarget]?.provider as string | undefined)
    : undefined

  // clod: updating the subscription reloads the config, mihomo rebuilds every
  // adapter and its delay history starts empty — the ping the user was looking
  // at turned into a placeholder word for a second or two and came back. So we
  // keep showing the last figure we saw until a new one arrives.
  //
  // The key pins that figure to the entry it was measured for: the selected
  // node, its group, and the leaf a balancer currently lands on. A remembered
  // ping is never shown next to a different server — it disappears instead.
  const pingKey = current
    ? `${group?.name ?? ''}::${current}::${leaf ?? ''}`
    : ''
  useEffect(() => {
    if (!usableDelay(delay)) return
    lastKnownPing = { key: pingKey, delay }
  }, [pingKey, delay])
  const remembered =
    lastKnownPing?.key === pingKey ? lastKnownPing.delay : undefined
  // Only the caption and the bars use it — `hasPing` and `measuredAt` below
  // must keep speaking about what the core really has, or the auto re-ping
  // would take a remembered figure for a live one and stop measuring.
  const shownDelay = hasPing ? delay : remembered

  const groupName = group?.name
  const updatedAt = currentProfile?.updated ?? 0
  useEffect(() => {
    if (!groupName) return
    const key = `${groupName}|${updatedAt}`
    if (lastAutoDelayKey === key) return
    // подождать, пока ядро дожуёт свежий конфиг, и перепинговать сразу.
    // Тест идёт через useGroupDelayTest: keepFixed + восстановление выбора,
    // так что автоперепинговка после обновления подписки НЕ сбрасывает
    // выбранный сервер — она только меряет.
    const timer = window.setTimeout(() => {
      lastAutoDelayKey = key
      // Групповой тест меряет и наш узел — одиночному автопингу ниже здесь
      // делать нечего ближайшую минуту.
      lastAutoPingAt = Date.now()
      runGroupDelayTest(groupName).catch(() => {})
    }, 800)
    return () => window.clearTimeout(timer)
  }, [groupName, updatedAt, runGroupDelayTest])

  // clod: показанному пингу больше минуты — перемеряем ОДИН узел, тот самый,
  // чья цифра висит на экране. Возраст берём из самих данных (время замера в
  // истории ядра), а не из «сколько окно пролежало в трее»: свежесть цифры не
  // зависит от того, кто и почему её обновил, и то же условие спасает экран,
  // открытый после долгого простоя, и подключение на свежем конфиге, где
  // истории ещё нет вовсе.
  //
  // Именно один узел, а не групповой тест: групповой идёт по всем узлам
  // подписки — сотни запросов по факту разворачивания окна, ради одной цифры
  // на экране. Групповой остаётся за кнопкой «Тест» и за обновлением подписки
  // (эффект выше).
  //
  // `Date.now()` живёт внутри эффекта: в теле компонента он делает рендер
  // нечистым (`react-compiler`).
  useEffect(() => {
    if (!visible || !groupName || !pingTarget) return

    // Групповой автотест (обновление подписки) и этот одиночный делят один
    // замок `lastAutoPingAt`: кто успел первым, тот и меряет. Замок общий, а
    // не отдельный гейт «дождись группового»: модульная переменная не
    // реактивна, и ожидание её значения молча пропускало бы замер, когда
    // групповой тест не состоялся (ядро ещё не поднялось).
    let attempts = 0
    const measure = () => {
      const now = Date.now()
      if (measuredAt && now - measuredAt < PING_MAX_AGE_MS) return
      if (now - lastAutoPingAt < (hasPing ? PING_GAP_MS : PING_RETRY_MS)) return

      lastAutoPingAt = now
      attempts += 1
      delayManager
        .unifiedDelayCheck(
          pingTarget,
          urlFor(groupName),
          PING_TIMEOUT_MS,
          pingProvider,
        )
        // Замер уходит в историю ядра — перечитываем её, чтобы цифра на экране
        // сменилась там же, где живут остальные данные.
        .finally(() => refreshProxy().catch(() => {}))
        .catch(() => {})
    }

    // Небольшая пауза: показ экрана тянет за собой перечитывание ядра, и
    // свежий замер может приехать уже оттуда — тогда мерить нечего.
    const timer = window.setTimeout(measure, 600)
    // Пустое место на экране закрываем настойчиво: повтор живёт, только пока
    // цифры нет, и снимается сам, как только данные приедут (эффект
    // перезапустится уже с ней).
    const retry = hasPing
      ? undefined
      : window.setInterval(() => {
          if (attempts >= PING_RETRY_LIMIT) {
            window.clearInterval(retry)
            return
          }
          measure()
        }, PING_RETRY_MS)
    return () => {
      window.clearTimeout(timer)
      if (retry !== undefined) window.clearInterval(retry)
    }
  }, [
    visible,
    groupName,
    measuredAt,
    hasPing,
    pingTarget,
    pingProvider,
    urlFor,
    refreshProxy,
  ])

  // clod: серверов может не быть вовсе — панель отдала одни заглушки. Тогда
  // строка не притворяется выбором, а называет причину: тот же статус, что и
  // в шторке, только одной строкой.
  const {
    reason,
    show: noServers,
    onlySentinels,
  } = useNoServersStatus(currentProfile)
  // Настоящими серверами считаем только узлы, и по всему конфигу сразу:
  // `DIRECT` и служебные имена ядра остаются в группе и после чистки заглушек,
  // но подключаться к ним нечем. `proxies` ещё не загружены (старт приложения,
  // рестарт ядра) — молчим: отсутствие групп в этот момент ничего не значит.
  const listEmpty = Boolean(proxies) && !hasRealNodes(proxies)
  const statusRow = noServers && (listEmpty || onlySentinels)
  const refillDate = currentProfile?.refill_date
    ? dayjs(toUnixSeconds(currentProfile.refill_date) * 1000).format(
        'DD.MM.YYYY',
      )
    : undefined
  const statusCaption = statusRow
    ? reason === 'traffic' && refillDate
      ? t('home.components.serverStatus.row.traffic', { date: refillDate })
      : t(
          `home.components.serverStatus.row.${reason === 'traffic' ? 'trafficNoDate' : reason}`,
        )
    : undefined
  const statusColor =
    reason === 'expired'
      ? 'error.main'
      : reason === 'traffic' || reason === 'deviceLimit'
        ? 'warning.main'
        : 'text.secondary'

  // clod: описание сервера принадлежит узлу, на который цепочка приземлилась,
  // — у балансировщика своего описания нет. Имя узла оно вытесняет: оно и так
  // продублировано в заголовке строки, а задержка нужнее и остаётся на месте.
  const description =
    (leaf ? descriptions[leaf] : undefined) ??
    (current ? descriptions[current] : undefined)
  const subject = description ?? (leaf ? nameWithoutFlag(leaf) : undefined)
  // No ping and nothing remembered means the figure is unknown, and a dash
  // says so — the same one the drawer shows. The word "Server" only repeated
  // the row's own title and read as a state the row had switched into.
  const caption = usableDelay(shownDelay)
    ? subject
      ? `${subject} · ${shownDelay} ${t('home.components.serverSelect.ms')}`
      : `${shownDelay} ${t('home.components.serverSelect.ms')}`
    : (subject ?? '—')

  return (
    <Stack
      direction="row"
      onClick={onOpen}
      sx={{
        alignItems: 'center',
        gap: 1.5,
        px: 1.75,
        py: 1.5,
        // stretch to the column like every other card; an explicit
        // width:100% + padding overflowed it by the padding width
        alignSelf: 'stretch',
        boxSizing: 'border-box',
        borderRadius: '14px',
        cursor: 'pointer',
        bgcolor: 'background.paper',
        border: (theme) =>
          `1px solid ${
            statusRow
              ? reason === 'expired'
                ? theme.palette.error.main
                : reason === 'traffic'
                  ? theme.palette.warning.main
                  : theme.palette.divider
              : theme.palette.divider
          }`,
        '&:hover': { borderColor: 'primary.main' },
      }}
    >
      {flagName ? <CountryFlag name={flagName} size={26} /> : null}
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography noWrap sx={{ fontSize: 14, fontWeight: 700 }}>
          {current
            ? nameWithoutFlag(current)
            : statusRow
              ? t('home.components.serverStatus.noServers')
              : t('home.components.serverSelect.none')}
        </Typography>
        <Typography
          noWrap
          sx={{ fontSize: 12, display: 'block' }}
          color={statusRow ? statusColor : 'text.secondary'}
          title={statusRow ? undefined : description}
        >
          {statusCaption ?? caption}
        </Typography>
      </Box>

      {/* clod:latency-style — вид индикатора выбирает провайдер заголовком;
          без заголовка остаются наши полоски. */}
      {currentProfile?.latency_style === 'dot' ? (
        <SignalDot delay={shownDelay} />
      ) : currentProfile?.latency_style === 'number' ? (
        <LatencyNumber delay={shownDelay} />
      ) : (
        <SignalBars delay={shownDelay} />
      )}

      <IconButton
        size="small"
        aria-label={t('home.components.serverSelect.title')}
      >
        <ExpandMoreRoundedIcon />
      </IconButton>
    </Stack>
  )
}
