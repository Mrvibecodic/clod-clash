import { ChevronRightRounded, ExpandMoreRounded } from '@mui/icons-material'
import {
  Box,
  List,
  ListItem,
  ListItemButton,
  ListItemText,
  ListSubheader,
} from '@mui/material'
import CircularProgress from '@mui/material/CircularProgress'
import React, { ReactNode, useState } from 'react'

import isAsyncFunction from '@/utils/is-async-function'

interface ItemProps {
  label: ReactNode
  extra?: ReactNode
  children?: ReactNode
  secondary?: ReactNode
  onClick?: () => void | Promise<any>
  // clod:simple-settings — строка не открывает диалог, а раскрывает блок под
  // собой. Стрелка должна показывать это состояние, а не «есть куда перейти»;
  // сам ряд остаётся тем же ListItemButton — с фокусом и клавиатурой.
  expanded?: boolean
}

export const SettingItem: React.FC<ItemProps> = ({
  label,
  extra,
  children,
  secondary,
  onClick,
  expanded,
}) => {
  const clickable = !!onClick

  const primary = (
    <Box sx={{ display: 'flex', alignItems: 'center', fontSize: '14px' }}>
      <span>{label}</span>
      {extra ? extra : null}
    </Box>
  )

  const [isLoading, setIsLoading] = useState(false)
  const handleClick = () => {
    if (onClick) {
      if (isAsyncFunction(onClick)) {
        setIsLoading(true)
        onClick()!.finally(() => setIsLoading(false))
      } else {
        onClick()
      }
    }
  }

  return clickable ? (
    <ListItem disablePadding>
      <ListItemButton onClick={handleClick} disabled={isLoading}>
        <ListItemText primary={primary} secondary={secondary} />
        {isLoading ? (
          <CircularProgress color="inherit" size={20} />
        ) : expanded === undefined ? (
          <ChevronRightRounded />
        ) : (
          <ExpandMoreRounded
            sx={{
              transition: 'transform .2s',
              transform: expanded ? 'rotate(180deg)' : 'none',
            }}
          />
        )}
      </ListItemButton>
    </ListItem>
  ) : (
    <ListItem sx={{ pt: '5px', pb: '5px' }}>
      <ListItemText primary={primary} secondary={secondary} />
      {children}
    </ListItem>
  )
}

// clod:simple-settings — заголовок необязателен. В простом режиме секция может
// быть продолжением предыдущей карточки или содержимым раскрывающегося блока
// «Продвинутые настройки», и второй заголовок с тем же именем там только мешает.
export const SettingList: React.FC<{
  title?: string
  children: ReactNode
}> = ({ title, children }) => (
  <List>
    {title ? (
      <ListSubheader
        sx={[
          { background: 'transparent', fontSize: '16px', fontWeight: '700' },
          ({ palette }) => {
            return {
              color: palette.text.primary,
            }
          },
        ]}
        disableSticky
      >
        {title}
      </ListSubheader>
    ) : null}

    {children}
  </List>
)
