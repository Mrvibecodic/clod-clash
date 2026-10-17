/**
 * Esc и щелчок мимо окна при несохранённом окно не закрывают: выйти без
 * сохранения можно только «Отменой».
 */
export const dismissUnlessDirty = <T>(
  dirty: boolean | undefined,
  onClose: T,
) => (dirty ? undefined : onClose)
