import { styled, Box, Typography } from '@mui/material'
import { Rule } from 'tauri-plugin-mihomo-api'

const Item = styled(Box)(({ theme }) => ({
  display: 'flex',
  padding: '4px 16px',
  color: theme.palette.text.primary,
  // clod:design-v3 — строка отзывается на курсор: раньше список правил был
  // полотном без единого признака, что строка под указателем.
  transition: theme.transitions.create(['background-color'], {
    duration: theme.transitions.duration.short,
  }),
  '&:hover': { backgroundColor: theme.palette.action.hover },
}))

// clod:design-v3 — было 'primary' и 'secondary': MUI по таким путям отдаёт
// объект палитры, а не цвет, и два оттенка из пяти не рисовались вовсе.
const COLOR = [
  'primary.main',
  'secondary.main',
  'info.main',
  'warning.main',
  'success.main',
]

interface Props {
  value: Rule & { lineNo: number }
}

const parseColor = (text: string) => {
  if (text === 'REJECT' || text === 'REJECT-DROP') return 'error.main'
  if (text === 'DIRECT') return 'text.primary'

  let sum = 0
  for (let i = 0; i < text.length; i++) {
    sum += text.charCodeAt(i)
  }
  return COLOR[sum % COLOR.length]
}

const RuleItem = (props: Props) => {
  const { value } = props

  return (
    <Item sx={{ borderBottom: '1px solid var(--divider-color)' }}>
      <Typography
        color="text.secondary"
        variant="body2"
        sx={{
          lineHeight: 2,
          minWidth: 30,
          mr: 2.25,
          textAlign: 'center',
          fontVariantNumeric: 'tabular-nums',
        }}
      >
        {value.lineNo}
      </Typography>

      <Box sx={{ userSelect: 'text' }}>
        <Typography component="h6" variant="subtitle1" color="text.primary">
          {value.payload || '-'}
        </Typography>

        <Typography
          component="span"
          variant="body2"
          color="text.secondary"
          sx={{ mr: 3, minWidth: 120, display: 'inline-block' }}
        >
          {value.type}
        </Typography>

        <Typography
          component="span"
          variant="body2"
          color={parseColor(value.proxy)}
        >
          {value.proxy}
        </Typography>
      </Box>
    </Item>
  )
}

export default RuleItem
