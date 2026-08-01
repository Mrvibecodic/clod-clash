import { Box } from '@mui/material'

import { parseBannerText } from '@/utils/banner-text'

interface Props {
  text?: string
}

/**
 * clod: провайдерский текст с подсветкой слов (`#RRGGBB` вплотную к слову).
 *
 * Один компонент на все места, где показывается `announce` и `clod-promo`:
 * баннеры главного экрана и диалог лимита устройств. Непокрашенный текст
 * наследует цвет родителя, поэтому вид баннера не меняется.
 */
export const BannerText = ({ text }: Props) => (
  <>
    {parseBannerText(text ?? '').map((fragment) => (
      <Box
        component="span"
        key={fragment.start}
        sx={fragment.color ? { color: fragment.color, fontWeight: 600 } : null}
      >
        {fragment.text}
      </Box>
    ))}
  </>
)
