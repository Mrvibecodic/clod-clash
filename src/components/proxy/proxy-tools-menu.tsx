import AccessTimeRounded from '@mui/icons-material/AccessTimeRounded'
import FilterAltOffRounded from '@mui/icons-material/FilterAltOffRounded'
import FilterAltRounded from '@mui/icons-material/FilterAltRounded'
import MoreHorizRounded from '@mui/icons-material/MoreHorizRounded'
import MyLocationRounded from '@mui/icons-material/MyLocationRounded'
import SortByAlphaRounded from '@mui/icons-material/SortByAlphaRounded'
import SortRounded from '@mui/icons-material/SortRounded'
import VisibilityOffRounded from '@mui/icons-material/VisibilityOffRounded'
import VisibilityRounded from '@mui/icons-material/VisibilityRounded'
import WifiTetheringOffRounded from '@mui/icons-material/WifiTetheringOffRounded'
import WifiTetheringRounded from '@mui/icons-material/WifiTetheringRounded'
import {
  Badge,
  Box,
  IconButton,
  ListItemIcon,
  ListSubheader,
  Menu,
  MenuItem,
  type SvgIconProps,
} from '@mui/material'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ProxySortType } from './use-filter-sort'
import type { HeadState } from './use-head-state'

interface Props {
  headState: HeadState
  onHeadState: (val: Partial<HeadState>) => void
  onLocation: () => void
  /** Раскрыть группу перед действием — у шапки группы в режиме «Правила». */
  ensureOpen?: () => void
  /** Поле фильтра или адреса только что показано — поставить в него курсор. */
  onFieldShown?: () => void
  iconSize?: SvgIconProps['fontSize']
}

/** Кнопка «Дополнительно» на группе прокси: всё, кроме проверки задержки, — в меню с подписями. */
export const ProxyToolsMenu = ({
  headState,
  onHeadState,
  onLocation,
  ensureOpen,
  onFieldShown,
  iconSize,
}: Props) => {
  const { t } = useTranslation()
  const [anchorEl, setAnchorEl] = useState<HTMLElement | null>(null)
  const { showType, sortType, filterText, textState, testUrl } = headState
  // Точка на кнопке: в меню включено то, что меняет список или проверку.
  const changed = sortType !== 0 || !!filterText.trim() || !!testUrl?.trim()

  const sortLabels = [
    t('proxies.page.menu.sortDefault'),
    t('proxies.page.menu.sortDelay'),
    t('proxies.page.menu.sortName'),
  ]

  const sections: {
    title: string
    items: {
      key: string
      icon: ReactNode
      label: string
      value?: string
      mono?: boolean
      keepOpen?: boolean
      run: () => void
    }[]
  }[] = [
    {
      title: t('proxies.page.menu.view'),
      items: [
        {
          key: 'locate',
          icon: <MyLocationRounded fontSize="small" />,
          label: t('proxies.page.tooltips.locate'),
          run: () => {
            ensureOpen?.()
            onLocation()
          },
        },
        {
          key: 'sort',
          icon:
            sortType === 1 ? (
              <AccessTimeRounded fontSize="small" />
            ) : sortType === 2 ? (
              <SortByAlphaRounded fontSize="small" />
            ) : (
              <SortRounded fontSize="small" />
            ),
          label: t('proxies.page.menu.sort'),
          value: sortLabels[sortType],
          keepOpen: true,
          run: () => {
            ensureOpen?.()
            onHeadState({ sortType: ((sortType + 1) % 3) as ProxySortType })
          },
        },
        {
          key: 'showType',
          icon: showType ? (
            <VisibilityRounded fontSize="small" />
          ) : (
            <VisibilityOffRounded fontSize="small" />
          ),
          label: t('proxies.page.tooltips.showBasic'),
          value: showType
            ? t('shared.statuses.disabled')
            : t('shared.statuses.enabled'),
          keepOpen: true,
          run: () => {
            ensureOpen?.()
            onHeadState({ showType: !showType })
          },
        },
        {
          key: 'filter',
          icon:
            textState === 'filter' ? (
              <FilterAltRounded fontSize="small" />
            ) : (
              <FilterAltOffRounded fontSize="small" />
            ),
          label: t('proxies.page.tooltips.filter'),
          value: filterText.trim() || undefined,
          run: () => {
            if (textState !== 'filter') ensureOpen?.()
            onHeadState({ textState: textState === 'filter' ? null : 'filter' })
            onFieldShown?.()
          },
        },
      ],
    },
    {
      title: t('proxies.page.menu.check'),
      items: [
        {
          key: 'url',
          icon:
            textState === 'url' ? (
              <WifiTetheringRounded fontSize="small" />
            ) : (
              <WifiTetheringOffRounded fontSize="small" />
            ),
          label: t('proxies.page.tooltips.delayCheckUrl'),
          value: testUrl?.trim() || t('proxies.page.menu.notSet'),
          mono: !!testUrl?.trim(),
          run: () => {
            onHeadState({ textState: textState === 'url' ? null : 'url' })
            onFieldShown?.()
          },
        },
      ],
    },
  ]

  return (
    <>
      <IconButton
        size="small"
        color="inherit"
        title={t('proxies.page.tooltips.more')}
        onClick={(e) => {
          e.preventDefault()
          e.stopPropagation()
          setAnchorEl(e.currentTarget)
        }}
      >
        <Badge variant="dot" color="primary" invisible={!changed}>
          <MoreHorizRounded fontSize={iconSize} />
        </Badge>
      </IconButton>
      <Menu
        anchorEl={anchorEl}
        open={!!anchorEl}
        onClose={() => setAnchorEl(null)}
        // Меню в портале, но события всплывают по дереву React до строки
        // группы: клик её свернул бы, нажатие и фокус — подсветили бы.
        onClick={(e) => e.stopPropagation()}
        onMouseDown={(e) => e.stopPropagation()}
        onTouchStart={(e) => e.stopPropagation()}
        onFocus={(e) => e.stopPropagation()}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
        transformOrigin={{ vertical: 'top', horizontal: 'right' }}
        slotProps={{ paper: { sx: { minWidth: 280, maxWidth: 360 } } }}
      >
        {sections.flatMap((section) => [
          <ListSubheader
            key={section.title}
            disableSticky
            sx={{
              bgcolor: 'transparent',
              color: 'text.secondary',
              fontSize: 11.5,
              fontWeight: 700,
              letterSpacing: '0.5px',
              lineHeight: '28px',
              textTransform: 'uppercase',
            }}
          >
            {section.title}
          </ListSubheader>,
          ...section.items.map((item) => (
            <MenuItem
              key={item.key}
              dense
              onClick={() => {
                if (!item.keepOpen) setAnchorEl(null)
                item.run()
              }}
            >
              <ListItemIcon>{item.icon}</ListItemIcon>
              <Box component="span" sx={{ flex: 1, whiteSpace: 'nowrap' }}>
                {item.label}
              </Box>
              {item.value && (
                <Box
                  component="span"
                  title={item.value}
                  sx={{
                    ml: 2,
                    minWidth: 0,
                    maxWidth: 140,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                    fontSize: 12.5,
                    color: 'text.secondary',
                    fontFamily: item.mono ? 'monospace' : undefined,
                  }}
                >
                  {item.value}
                </Box>
              )}
            </MenuItem>
          )),
        ])}
      </Menu>
    </>
  )
}
