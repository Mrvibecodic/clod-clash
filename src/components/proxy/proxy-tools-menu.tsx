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
  IconButton,
  ListItemIcon,
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

  const items: {
    key: string
    icon: ReactNode
    label: string
    keepOpen?: boolean
    run: () => void
  }[] = [
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
      label: [
        t('proxies.page.tooltips.sortDefault'),
        t('proxies.page.tooltips.sortDelay'),
        t('proxies.page.tooltips.sortName'),
      ][sortType],
      keepOpen: true,
      run: () => {
        ensureOpen?.()
        onHeadState({ sortType: ((sortType + 1) % 3) as ProxySortType })
      },
    },
    {
      key: 'url',
      icon:
        textState === 'url' ? (
          <WifiTetheringRounded fontSize="small" />
        ) : (
          <WifiTetheringOffRounded fontSize="small" />
        ),
      label: t('proxies.page.tooltips.delayCheckUrl'),
      run: () => {
        onHeadState({ textState: textState === 'url' ? null : 'url' })
        onFieldShown?.()
      },
    },
    {
      key: 'showType',
      icon: showType ? (
        <VisibilityRounded fontSize="small" />
      ) : (
        <VisibilityOffRounded fontSize="small" />
      ),
      label: showType
        ? t('proxies.page.tooltips.showBasic')
        : t('proxies.page.tooltips.showDetail'),
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
      run: () => {
        if (textState !== 'filter') ensureOpen?.()
        onHeadState({ textState: textState === 'filter' ? null : 'filter' })
        onFieldShown?.()
      },
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
      >
        {items.map((item) => (
          <MenuItem
            key={item.key}
            dense
            onClick={() => {
              if (!item.keepOpen) setAnchorEl(null)
              item.run()
            }}
          >
            <ListItemIcon>{item.icon}</ListItemIcon>
            {item.label}
          </MenuItem>
        ))}
      </Menu>
    </>
  )
}
