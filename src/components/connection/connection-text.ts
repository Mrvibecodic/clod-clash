import { alpha, type Theme } from '@mui/material'

const MONO = 'monospace'
const DIRECT = 'DIRECT'

export const connTextSx = ({ palette }: Theme) => ({
  '& .cc-mono': { fontFamily: MONO },
  '& .cc-cut': {
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  '& .cc-muted': { color: palette.text.disabled },
  '& .cc-sec': { color: palette.text.secondary },
  '& .cc-dn': { color: palette.primary.main },
  '& .cc-up': { color: palette.secondary.main },
  '& .cc-direct': { color: palette.success.main },
  '& .cc-reject': { color: palette.error.main },
  '& .cc-chain': {
    display: 'flex',
    alignItems: 'center',
    gap: '5px',
    minWidth: 0,
    overflow: 'hidden',
    whiteSpace: 'nowrap',
  },
  '& .cc-hop': {
    flex: '0 3 auto',
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    color: palette.text.secondary,
  },
  '& .cc-sep': { color: palette.text.disabled, flex: 'none' },
  '& .cc-last': {
    minWidth: 0,
    fontWeight: 600,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  '& .cc-rule': {
    display: 'flex',
    alignItems: 'center',
    gap: '7px',
    minWidth: 0,
    overflow: 'hidden',
    whiteSpace: 'nowrap',
  },
  '& .cc-tchip': {
    flex: 'none',
    padding: '3px 6px',
    borderRadius: '5px',
    backgroundColor: alpha(palette.primary.main, 0.14),
    color: palette.primary.main,
    fontFamily: MONO,
    fontSize: 11,
    fontWeight: 600,
    lineHeight: 1.3,
  },
  '& .cc-pl': {
    minWidth: 0,
    fontFamily: MONO,
    fontSize: 12.5,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  '& .cc-rule.cc-wrap': { alignItems: 'flex-start', whiteSpace: 'normal' },
  '& .cc-wrap .cc-pl': { wordBreak: 'break-all' },
})

export const connTileSx = ({ palette }: Theme) => ({
  cursor: 'pointer',
  bgcolor: alpha(palette.text.primary, 0.045),
  transition: 'background-color 120ms cubic-bezier(0.2, 0, 0, 1)',
  '&:hover': { bgcolor: alpha(palette.text.primary, 0.08) },
  '&[data-selected="true"]': {
    bgcolor: alpha(palette.primary.main, 0.14),
    boxShadow: `inset 3px 0 0 ${palette.primary.main}`,
  },
})

export const outboundClass = (name: string) =>
  name === DIRECT ? 'cc-direct' : name.startsWith('REJECT') ? 'cc-reject' : ''

export const outboundName = (name: string, directLabel: string) =>
  name === DIRECT ? directLabel : name
