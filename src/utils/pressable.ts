import type { KeyboardEvent } from 'react'

/**
 * Нажимаемый блок для клавиатуры и экранного диктора: фокус по Tab, роль
 * кнопки, Enter и пробел. Клавиши вложенных кнопок блок не перехватывает.
 */
export const pressable = (onPress: () => void) => ({
  role: 'button' as const,
  tabIndex: 0,
  onKeyDown: (event: KeyboardEvent) => {
    if (event.target !== event.currentTarget) return
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      onPress()
    }
  },
})
