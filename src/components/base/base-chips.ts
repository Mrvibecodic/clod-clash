import { alpha, styled } from '@mui/material'

const chipBase = {
  display: 'inline-block',
  padding: '2px 6px',
  borderRadius: 5,
  fontFamily: 'monospace',
  fontSize: 11,
  lineHeight: 1.35,
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  verticalAlign: 'middle',
} as const

export const TypeChip = styled('span')(({ theme }) => ({
  ...chipBase,
  flex: 'none',
  backgroundColor: alpha(theme.palette.primary.main, 0.14),
  color: theme.palette.primary.main,
  fontWeight: 600,
}))

export const CodeChip = styled('span')(({ theme }) => ({
  ...chipBase,
  backgroundColor: alpha(theme.palette.text.primary, 0.06),
  color: theme.palette.text.secondary,
}))
