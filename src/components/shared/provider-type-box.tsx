import { Box, alpha, styled } from '@mui/material'

/** Плашка у имени провайдера: число узлов или правил, тип, поведение. */
export const TypeBox = styled(Box)<{ component?: React.ElementType }>(
  ({ theme }) => ({
    display: 'inline-block',
    border: '1px solid #ccc',
    borderColor: alpha(theme.palette.secondary.main, 0.5),
    color: alpha(theme.palette.secondary.main, 0.8),
    borderRadius: 4,
    fontSize: 10,
    marginRight: '4px',
    padding: '0 2px',
    lineHeight: 1.25,
  }),
)
