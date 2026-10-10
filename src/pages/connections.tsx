import {
  CheckRounded,
  DeleteForeverRounded,
  InsightsRounded,
  TableChartRounded,
  TableRowsRounded,
  ViewColumnRounded,
} from '@mui/icons-material'
import {
  Box,
  Button,
  Fab,
  IconButton,
  MenuItem,
  Tooltip,
  useMediaQuery,
  Zoom,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useCallback, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import {
  BaseEmpty,
  BasePage,
  BaseSearchBox,
  BaseSegmented,
  BaseStyledSelect,
  type SearchState,
  VirtualList,
} from '@/components/base'
import {
  ConnectionDetail,
  ConnectionDetailRef,
} from '@/components/connection/connection-detail'
import { ConnectionRowItem } from '@/components/connection/connection-row-item'
import {
  getConnectionStartTime,
  useConnectionRowViews,
} from '@/components/connection/connection-row-view'
import { ConnectionSummary } from '@/components/connection/connection-summary'
import {
  ConnectionTable,
  type ConnectionTableCollapsed,
  type ConnectionTableSorting,
} from '@/components/connection/connection-table'
import { connTextSx } from '@/components/connection/connection-text'
import { useConnectionData } from '@/hooks/use-connection-data'
import { useConnectionSetting } from '@/hooks/use-connection-setting'
import { useTrafficData } from '@/hooks/use-traffic-data'
import { useVisibility } from '@/hooks/use-visibility'
import { NARROW_QUERY } from '@/pages/_theme'
import { showNotice } from '@/services/notice-service'
import parseTraffic from '@/utils/parse-traffic'

type OrderFunc = (list: IConnectionsItem[]) => IConnectionsItem[]

const ORDER_OPTIONS = [
  {
    id: 'default',
    labelKey: 'connections.components.order.default',
    fn: (list: IConnectionsItem[]) =>
      list.sort(
        (a, b) => getConnectionStartTime(b) - getConnectionStartTime(a),
      ),
  },
  {
    id: 'uploadSpeed',
    labelKey: 'connections.components.order.uploadSpeed',
    fn: (list: IConnectionsItem[]) =>
      list.sort((a, b) => (b.curUpload ?? 0) - (a.curUpload ?? 0)),
  },
  {
    id: 'downloadSpeed',
    labelKey: 'connections.components.order.downloadSpeed',
    fn: (list: IConnectionsItem[]) =>
      list.sort((a, b) => (b.curDownload ?? 0) - (a.curDownload ?? 0)),
  },
] as const

type OrderKey = (typeof ORDER_OPTIONS)[number]['id']

const GROUP_OPTIONS = [
  { id: 'none', labelKey: 'connections.components.group.none' },
  { id: 'process', labelKey: 'connections.components.group.process' },
  { id: 'chain', labelKey: 'connections.components.group.chain' },
  { id: 'rule', labelKey: 'connections.components.group.rule' },
] as const satisfies readonly { id: IConnectionGroupBy; labelKey: string }[]

const orderFunctionMap = ORDER_OPTIONS.reduce<Record<OrderKey, OrderFunc>>(
  (acc, option) => {
    acc[option.id] = option.fn
    return acc
  },
  {} as Record<OrderKey, OrderFunc>,
)

const EMPTY_CONNECTIONS: IConnectionsItem[] = []

const selectLabel = (label: string, value: string) => (
  <>
    <Box component="span" sx={{ color: 'text.secondary', mr: 0.75 }}>
      {label}:
    </Box>
    {value}
  </>
)

const menuCheck = (on: boolean) =>
  on ? (
    <CheckRounded
      fontSize="small"
      sx={{ ml: 'auto', pl: 1.5, color: 'primary.main' }}
    />
  ) : null

const SELECT_SX = {
  width: 'auto',
  minWidth: 200,
  maxWidth: '100%',
  fontSize: 14,
}

const menuItemSx = { display: 'flex', alignItems: 'center', fontSize: 14 }
const ConnectionsPage = () => {
  const { t } = useTranslation()
  const pageVisible = useVisibility({ keepWhileMinimized: true })
  const [match, setMatch] = useState<(input: string) => boolean>(
    () => () => true,
  )
  const [hasSearch, setHasSearch] = useState(false)
  const [curOrderOpt, setCurOrderOpt] = useState<OrderKey>('default')
  const [connectionsType, setConnectionsType] = useState<'active' | 'closed'>(
    'active',
  )

  const {
    response: { data: connections },
    clearClosedConnections,
  } = useConnectionData({ enabled: pageVisible })
  const {
    response: { data: traffic },
  } = useTrafficData({ enabled: pageVisible })

  const [setting, setSetting] = useConnectionSetting()

  const narrow = useMediaQuery(NARROW_QUERY, { noSsr: true })
  const isTableLayout = setting.layout === 'table' && !narrow
  const groupBy = setting.groupBy ?? 'none'
  const [tableSorting, setTableSorting] = useState<ConnectionTableSorting>(null)
  const [tableCollapsed, setTableCollapsed] =
    useState<ConnectionTableCollapsed>(null)
  const [detailId, setDetailId] = useState<string | null>(null)
  const summaryVisible = setting.summary ?? true

  const [isColumnManagerOpen, setIsColumnManagerOpen] = useState(false)
  const [tableShown, setTableShown] = useState(isTableLayout)
  if (tableShown !== isTableLayout) {
    setTableShown(isTableLayout)
    if (!isTableLayout) setIsColumnManagerOpen(false)
  }

  const selectedConnections =
    connectionsType === 'active'
      ? (connections?.activeConnections ?? EMPTY_CONNECTIONS)
      : (connections?.closedConnections ?? EMPTY_CONNECTIONS)

  const filterConn = useMemo(() => {
    const orderFunc = orderFunctionMap[curOrderOpt]

    if (isTableLayout && !hasSearch) return selectedConnections
    if (!hasSearch) return orderFunc([...selectedConnections])

    const matchConns = selectedConnections.filter((conn) => {
      const { host, destinationIP, process } = conn.metadata
      return (
        match(host || '') || match(destinationIP || '') || match(process || '')
      )
    })

    return orderFunc ? orderFunc(matchConns) : matchConns
  }, [selectedConnections, isTableLayout, hasSearch, match, curOrderOpt])

  const displayRows = useConnectionRowViews(
    isTableLayout ? EMPTY_CONNECTIONS : filterConn,
  )

  const detailRef = useRef<ConnectionDetailRef>(null!)

  const selectConnectionsType = useCallback(
    (type: 'active' | 'closed') => {
      if (type === connectionsType) return
      detailRef.current?.close()
      setIsColumnManagerOpen(false)
      setConnectionsType(type)
    },
    [connectionsType],
  )

  const showDetailById = useCallback((id: string) => {
    detailRef.current?.open(id)
  }, [])

  const onCloseAll = useLockFn(async () => {
    try {
      await closeAllConnections()
    } catch (err) {
      showNotice.error(err)
    }
  })

  const handleSearch = useCallback(
    (match: (content: string) => boolean, state: SearchState) => {
      setMatch(() => match)
      setHasSearch(state.text.length > 0)
    },
    [],
  )
  const hasTableData = filterConn.length > 0

  return (
    <BasePage
      full
      title={t('connections.page.title')}
      contentStyle={{
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
        borderRadius: '8px',
        minHeight: 0,
      }}
      header={
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 2 }}>
          <Box
            sx={{
              display: 'flex',
              flexDirection: { xs: 'column', md: 'row' },
              alignItems: 'flex-end',
              columnGap: 3,
              mx: 1,
              fontSize: { xs: 12, md: 13 },
              lineHeight: 1.35,
              whiteSpace: 'nowrap',
            }}
          >
            <span>
              <Box component="span" sx={{ color: 'text.secondary', mr: 0.75 }}>
                {t('shared.labels.downloaded')}
              </Box>
              <b>{parseTraffic(traffic?.downTotal || 0)}</b>
            </span>
            <span>
              <Box component="span" sx={{ color: 'text.secondary', mr: 0.75 }}>
                {t('shared.labels.uploaded')}
              </Box>
              <b>{parseTraffic(traffic?.upTotal || 0)}</b>
            </span>
          </Box>
          <IconButton
            color="inherit"
            size="small"
            onClick={() =>
              setSetting((o) => ({
                ...(o ?? { layout: 'table' }),
                summary: !(o?.summary ?? true),
              }))
            }
          >
            <InsightsRounded
              titleAccess={t('connections.components.summary.toggle')}
              sx={{ opacity: summaryVisible ? 1 : 0.45 }}
            />
          </IconButton>
          {narrow ? null : (
            <IconButton
              color="inherit"
              size="small"
              onClick={() =>
                setSetting((o) =>
                  o?.layout !== 'table'
                    ? { ...o, layout: 'table' }
                    : { ...o, layout: 'list' },
                )
              }
            >
              {isTableLayout ? (
                <TableRowsRounded titleAccess={t('shared.actions.listView')} />
              ) : (
                <TableChartRounded
                  titleAccess={t('shared.actions.tableView')}
                />
              )}
            </IconButton>
          )}
          <Button
            size="small"
            variant="outlined"
            color="error"
            onClick={onCloseAll}
          >
            <span style={{ whiteSpace: 'nowrap' }}>
              {t('shared.actions.closeAll')}
            </span>
          </Button>
        </Box>
      }
    >
      {summaryVisible && hasTableData && (
        <ConnectionSummary
          connections={filterConn}
          closed={connectionsType === 'closed'}
        />
      )}
      <Box
        sx={{
          pt: 1,
          mb: 0.5,
          mx: '10px',
          minHeight: '36px',
          display: 'flex',
          flexWrap: 'wrap',
          alignItems: 'center',
          gap: 1,
          userSelect: 'text',
          position: 'sticky',
          top: 0,
          zIndex: 2,
        }}
      >
        <BaseSegmented
          value={connectionsType}
          onChange={selectConnectionsType}
          options={[
            {
              value: 'active',
              label: (
                <>
                  {t('connections.components.actions.active')}
                  <Box component="span" sx={{ ml: 0.75, opacity: 0.7 }}>
                    {connections?.activeConnections.length}
                  </Box>
                </>
              ),
            },
            {
              value: 'closed',
              label: (
                <>
                  {t('connections.components.actions.closed')}
                  <Box component="span" sx={{ ml: 0.75, opacity: 0.7 }}>
                    {connections?.closedConnections.length}
                  </Box>
                </>
              ),
            },
          ]}
        />
        {isTableLayout ? (
          <BaseStyledSelect
            sx={SELECT_SX}
            value={groupBy}
            renderValue={(value) =>
              selectLabel(
                t('connections.components.group.label'),
                t(
                  GROUP_OPTIONS.find((option) => option.id === value)
                    ?.labelKey ?? GROUP_OPTIONS[0].labelKey,
                ),
              )
            }
            onChange={(e) =>
              setSetting((o) => ({
                ...(o ?? { layout: 'table' }),
                groupBy: e.target.value as IConnectionGroupBy,
              }))
            }
          >
            {GROUP_OPTIONS.map((option) => (
              <MenuItem key={option.id} value={option.id} sx={menuItemSx}>
                {t(option.labelKey)}
                {menuCheck(option.id === groupBy)}
              </MenuItem>
            ))}
          </BaseStyledSelect>
        ) : (
          <BaseStyledSelect
            sx={SELECT_SX}
            value={curOrderOpt}
            renderValue={(value) =>
              selectLabel(
                t('connections.components.order.label'),
                t(
                  ORDER_OPTIONS.find((option) => option.id === value)
                    ?.labelKey ?? ORDER_OPTIONS[0].labelKey,
                ),
              )
            }
            onChange={(e) => setCurOrderOpt(e.target.value as OrderKey)}
          >
            {ORDER_OPTIONS.map((option) => (
              <MenuItem key={option.id} value={option.id} sx={menuItemSx}>
                {t(option.labelKey)}
                {menuCheck(option.id === curOrderOpt)}
              </MenuItem>
            ))}
          </BaseStyledSelect>
        )}
        <Box
          sx={{
            flex: '1 1 220px',
            display: 'flex',
            alignItems: 'center',
            '& > *': {
              flex: 1,
            },
          }}
        >
          <BaseSearchBox onSearch={handleSearch} />
        </Box>
        {isTableLayout && hasTableData && (
          <Tooltip title={t('connections.components.columnManager.title')}>
            <IconButton
              size="small"
              aria-label={t('connections.components.columnManager.title')}
              onClick={() => setIsColumnManagerOpen(true)}
              sx={{ flex: '0 0 auto' }}
            >
              <ViewColumnRounded fontSize="small" />
            </IconButton>
          </Tooltip>
        )}
      </Box>

      {!hasTableData ? (
        <BaseEmpty />
      ) : isTableLayout ? (
        <ConnectionTable
          connections={filterConn}
          groupBy={groupBy}
          sorting={tableSorting}
          onSortingChange={setTableSorting}
          collapsed={tableCollapsed}
          onCollapsedChange={setTableCollapsed}
          onShowDetail={showDetailById}
          selectedId={detailId}
          columnManagerOpen={isColumnManagerOpen}
          onCloseColumnManager={() => setIsColumnManagerOpen(false)}
        />
      ) : (
        <Box
          sx={[
            connTextSx,
            { flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' },
          ]}
        >
          <VirtualList
            key={connectionsType}
            count={displayRows.length}
            estimateSize={58}
            renderItem={(i) => (
              <ConnectionRowItem
                row={displayRows[i]}
                closed={connectionsType === 'closed'}
                selected={displayRows[i].id === detailId}
                onShowDetail={showDetailById}
              />
            )}
            style={{
              flex: 1,
              padding: '0 10px',
              WebkitOverflowScrolling: 'touch',
              overscrollBehavior: 'contain',
            }}
          />
        </Box>
      )}
      <ConnectionDetail
        ref={detailRef}
        activeConnections={connections.activeConnections}
        closedConnections={connections.closedConnections}
        onOpenChange={setDetailId}
      />
      <Zoom
        in={connectionsType === 'closed' && filterConn.length > 0}
        unmountOnExit
      >
        <Fab
          size="medium"
          variant="extended"
          sx={{
            position: 'absolute',
            right: 16,
            bottom: isTableLayout ? 70 : 16,
          }}
          color="primary"
          onClick={() => clearClosedConnections()}
        >
          <DeleteForeverRounded sx={{ mr: 1 }} fontSize="small" />
          {t('shared.actions.clear')}
        </Fab>
      </Zoom>
    </BasePage>
  )
}

export default ConnectionsPage
