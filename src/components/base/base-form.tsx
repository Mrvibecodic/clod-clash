import {
  alpha,
  Box,
  type SxProps,
  type Theme,
  ToggleButton,
  ToggleButtonGroup,
} from '@mui/material'
import type { ReactNode } from 'react'

const toSxArray = (sx: SxProps<Theme> | undefined) =>
  Array.isArray(sx) ? sx : [sx ?? false]

export const FormSection = ({
  title,
  count,
  hint,
  extra,
}: {
  title: ReactNode
  count?: number
  hint?: ReactNode
  extra?: ReactNode
}) => (
  <Box
    sx={{
      position: 'sticky',
      top: 'var(--form-section-top, 0px)',
      zIndex: 2,
      display: 'flex',
      alignItems: 'center',
      gap: 1,
      minHeight: 34,
      pt: 1.25,
      pb: 0.5,
      px: 0.25,
      bgcolor: 'background.paper',
      backgroundImage: 'var(--Paper-overlay)',
    }}
  >
    <Box component="span" sx={{ fontSize: 12.5, fontWeight: 700 }}>
      {title}
    </Box>
    {count !== undefined ? (
      <Box
        component="span"
        sx={{ fontSize: 12.5, fontWeight: 600, color: 'text.secondary' }}
      >
        {count}
      </Box>
    ) : null}
    {hint ? (
      <Box
        component="span"
        sx={{
          ml: 'auto',
          fontSize: 12.5,
          color: 'text.secondary',
          textAlign: 'right',
        }}
      >
        {hint}
      </Box>
    ) : null}
    {extra ? <Box sx={{ ml: hint ? 0 : 'auto' }}>{extra}</Box> : null}
  </Box>
)

export const FormRow = ({
  label,
  help,
  disabled,
  children,
}: {
  label: ReactNode
  help?: ReactNode
  disabled?: boolean
  children?: ReactNode
}) => (
  <Box
    sx={{
      display: 'flex',
      alignItems: 'center',
      gap: 1.75,
      py: 1.125,
      '& + &': { borderTop: 1, borderColor: 'divider' },
    }}
  >
    <Box sx={{ flex: 1, minWidth: 0 }}>
      <Box
        sx={{
          fontSize: 14,
          color: disabled ? 'text.secondary' : 'text.primary',
          transition: 'color 150ms',
        }}
      >
        {label}
      </Box>
      {help ? (
        <Box sx={{ mt: 0.5, fontSize: 12.5, color: 'text.secondary' }}>
          {help}
        </Box>
      ) : null}
    </Box>
    {children !== undefined ? (
      <Box
        sx={{ flex: 'none', display: 'flex', alignItems: 'center', gap: 0.75 }}
      >
        {children}
      </Box>
    ) : null}
  </Box>
)

export const FormField = ({
  label,
  optional,
  help,
  children,
  sx,
}: {
  label: ReactNode
  optional?: ReactNode
  help?: ReactNode
  children: ReactNode
  sx?: SxProps<Theme>
}) => (
  <Box sx={[{ py: 1.125, minWidth: 0 }, ...toSxArray(sx)]}>
    <Box sx={{ mb: 0.75, fontSize: 13, fontWeight: 600 }}>
      {label}
      {optional ? (
        <Box
          component="span"
          sx={{ ml: 0.75, fontWeight: 400, color: 'text.secondary' }}
        >
          {optional}
        </Box>
      ) : null}
    </Box>
    {children}
    {help ? (
      <Box sx={{ mt: 0.75, fontSize: 12.5, color: 'text.secondary' }}>
        {help}
      </Box>
    ) : null}
  </Box>
)

export const FormHint = ({
  children,
  sx,
}: {
  children: ReactNode
  sx?: SxProps<Theme>
}) => (
  <Box
    sx={[
      {
        border: '1px dashed',
        borderColor: 'divider',
        borderRadius: '10px',
        px: 1.5,
        py: 1.25,
        fontSize: 13,
        color: 'text.secondary',
      },
      ...toSxArray(sx),
    ]}
  >
    {children}
  </Box>
)

export const FormTile = ({
  children,
  selected,
  onClick,
  sx,
}: {
  children: ReactNode
  selected?: boolean
  onClick?: () => void
  sx?: SxProps<Theme>
}) => (
  <Box
    onClick={onClick}
    sx={[
      ({ palette }) => ({
        display: 'flex',
        alignItems: 'center',
        gap: 1.25,
        minHeight: 42,
        my: 0.625,
        pl: 1.5,
        pr: 0.75,
        py: 0.5,
        borderRadius: '10px',
        bgcolor: selected
          ? alpha(palette.primary.main, 0.14)
          : alpha(palette.text.primary, 0.07),
        boxShadow: selected
          ? `inset 3px 0 0 ${palette.primary.main}`
          : undefined,
        cursor: onClick ? 'pointer' : undefined,
        transition: 'background-color 150ms',
        '&:hover': onClick
          ? {
              bgcolor: selected
                ? alpha(palette.primary.main, 0.18)
                : alpha(palette.text.primary, 0.1),
            }
          : undefined,
      }),
      ...toSxArray(sx),
    ]}
  >
    {children}
  </Box>
)

export interface SegmentedOption<T extends string> {
  value: T
  label: ReactNode
}

export const BaseSegmented = <T extends string>({
  value,
  options,
  onChange,
  fullWidth,
  disabled,
  sx,
}: {
  value: T
  options: readonly SegmentedOption<T>[]
  onChange: (next: T) => void
  fullWidth?: boolean
  disabled?: boolean
  sx?: SxProps<Theme>
}) => (
  <ToggleButtonGroup
    exclusive
    size="small"
    color="primary"
    fullWidth={fullWidth}
    disabled={disabled}
    value={value}
    onChange={(_, next: T | null) => {
      if (next !== null && next !== value) onChange(next)
    }}
    sx={[
      {
        '& .MuiToggleButton-root': {
          textTransform: 'none',
          px: 1.5,
          py: 0.5,
          whiteSpace: 'nowrap',
          transition: 'background-color 150ms, color 150ms',
        },
      },
      ...toSxArray(sx),
    ]}
  >
    {options.map((option) => (
      <ToggleButton key={option.value} value={option.value}>
        {option.label}
      </ToggleButton>
    ))}
  </ToggleButtonGroup>
)
