/**
 * Простой или расширенный интерфейс: выбор человека, затем подсказка
 * провайдера (`clod-simple-mode`), иначе простой. Провайдер может предложить
 * режим тем, кто не открывал настройки, но не отнимет расширенный у того, кто
 * его выбрал.
 */
export const resolveSimpleMode = (
  userChoice: boolean | null | undefined,
  providerChoice: boolean | null | undefined,
) => userChoice ?? providerChoice ?? true
