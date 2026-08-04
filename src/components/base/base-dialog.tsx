import {
  Button,
  Dialog,
  type DialogProps,
  DialogActions,
  DialogContent,
  DialogTitle,
  type SxProps,
  type Theme,
} from '@mui/material'
import { ReactNode } from 'react'

interface Props {
  title: ReactNode
  open: boolean
  okBtn?: ReactNode
  cancelBtn?: ReactNode
  disableEnforceFocus?: boolean
  disableOk?: boolean
  disableCancel?: boolean
  disableFooter?: boolean
  contentSx?: SxProps<Theme>
  /** clod: узкое окно простого режима — диалог должен считаться от него. */
  fullWidth?: boolean
  maxWidth?: DialogProps['maxWidth']
  paperSx?: SxProps<Theme>
  children?: ReactNode
  loading?: boolean
  onOk?: () => void
  onCancel?: () => void
  onClose?: () => void
}

export interface DialogRef {
  open: () => void
  close: () => void
}

/**
 * clod: диалог не должен вылезать за окно — ни на одном экране приложения.
 *
 * Приложение живёт без CssBaseline, то есть без глобального
 * `box-sizing: border-box`: у `DialogContent` ширина считается ПО КОНТЕНТУ,
 * а собственные поля (24 px слева и справа) прибавляются сверху. Поэтому
 * `contentSx={{ width: 550 }}` — это 598 px реальной ширины, и в окне
 * простого режима (560 px) содержимое уезжало за правый край: строки
 * обрезались, кнопки уходили за границу, внизу появлялась горизонтальная
 * прокрутка. Ровно это и случилось с окном «Новая версия».
 *
 * Правила ниже страхуют ЛЮБОЙ диалог, а не только тот, где заметили:
 * содержимое никогда не шире бумаги за вычетом полей, длинное слово в
 * заголовке переносится, а ряд кнопок уходит на вторую строку вместо
 * выезда за край. Сама бумага в окно вписывается и без нас: у неё
 * `overflow-y: auto`, поэтому флекс ужимает её до ширины окна.
 */
const TITLE_FIT = {
  minWidth: 0,
  overflowWrap: 'anywhere',
} as const

// 48 px = левое + правое поле DialogContent (24 px). Горизонтальные поля
// нигде в проекте не переопределяются — при правке проверить вызовы.
const CONTENT_FIT = {
  maxWidth: 'calc(100% - 48px)',
} as const

const ACTIONS_FIT = {
  flexWrap: 'wrap',
} as const

// Последняя страховка от горизонтальной прокрутки: что бы ни насчитал вызов
// (своя ширина, поля, длинное слово), бумага остаётся флекс-элементом, который
// МОЖЕТ ужаться до окна — `minWidth: 0` снимает запрет сжатия ниже ширины
// содержимого, — а полосы прокрутки по горизонтали у неё не бывает вовсе.
// Вертикальную это не трогает: `overflow-y: auto` MUI ставит сам.
const PAPER_FIT = {
  minWidth: 0,
  overflowX: 'hidden',
} as const

// `false` вместо пустого значения: MUI пропускает такой элемент массива sx.
const toSxArray = (sx: SxProps<Theme> | undefined) =>
  Array.isArray(sx) ? sx : [sx ?? false]

export const BaseDialog: React.FC<Props> = ({
  open,
  title,
  children,
  okBtn,
  cancelBtn,
  disableEnforceFocus,
  contentSx,
  fullWidth,
  maxWidth,
  paperSx,
  disableCancel,
  disableOk,
  disableFooter,
  loading,
  onOk,
  onCancel,
  onClose,
}) => {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      disableEnforceFocus={disableEnforceFocus}
      fullWidth={fullWidth}
      maxWidth={maxWidth}
      slotProps={{ paper: { sx: [PAPER_FIT, ...toSxArray(paperSx)] } }}
    >
      <DialogTitle sx={TITLE_FIT}>{title}</DialogTitle>

      <DialogContent sx={[CONTENT_FIT, ...toSxArray(contentSx)]}>
        {children}
      </DialogContent>

      {!disableFooter && (
        <DialogActions sx={ACTIONS_FIT}>
          {!disableCancel && (
            <Button variant="outlined" onClick={onCancel}>
              {cancelBtn}
            </Button>
          )}
          {!disableOk && (
            <Button loading={loading} variant="contained" onClick={onOk}>
              {okBtn}
            </Button>
          )}
        </DialogActions>
      )}
    </Dialog>
  )
}
