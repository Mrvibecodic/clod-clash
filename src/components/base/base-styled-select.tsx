import { Select, SelectProps, styled } from '@mui/material'

export const BaseStyledSelect = styled(
  ({ sx, ...props }: SelectProps<string>) => {
    return (
      <Select
        size="small"
        autoComplete="new-password"
        // Свой `sx` идёт вторым слоем: вызывающий может расширить ширину под
        // длинную подпись, не теряя высоту и отступы базового селекта.
        sx={[
          {
            width: 120,
            height: 33.375,
            mr: 1,
            '[role="button"]': { py: 0.65 },
          },
          ...(Array.isArray(sx) ? sx : [sx]),
        ]}
        {...props}
      />
    )
  },
)(({ theme }) => ({
  background: theme.palette.mode === 'light' ? '#fff' : undefined,
}))
