import { Box } from '@mui/material'
import type { ReactNode } from 'react'

type Props = {
  label: string
  padding?: string
  children?: ReactNode
}

export const BaseFieldset: React.FC<Props> = ({
  label,
  padding,
  children,
}: Props) => {
  const fieldsetPadding = padding ?? '15px'

  return (
    <Box
      component="fieldset"
      sx={{
        position: 'relative',
        border: '1px solid #bbb',
        borderRadius: '5px',
        width: 'auto',
        padding: fieldsetPadding,
      }}
    >
      <Box
        component="legend"
        sx={{
          position: 'absolute',
          top: '-10px',
          left: fieldsetPadding,
          backgroundColor: 'background.paper',
          backgroundImage:
            'linear-gradient(rgba(255, 255, 255, 0.16), rgba(255, 255, 255, 0.16))',
          color: 'text.primary',
          fontSize: '1em',
        }}
      >
        {label}
      </Box>
      {children}
    </Box>
  )
}
