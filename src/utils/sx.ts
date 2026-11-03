import type { SxProps, Theme } from '@mui/material'

/** `sx` вызывающего — вторым слоем; `false` вместо пустого: MUI пропускает такой элемент. */
export const toSxArray = (sx: SxProps<Theme> | undefined) =>
  Array.isArray(sx) ? sx : [sx ?? false]
