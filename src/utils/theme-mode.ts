/**
 * Тема окна из настроек. Нет ключа в файле настроек — «как в системе», как и
 * у окна на старте; `undefined` — настройки ещё не прочитаны.
 */
export const themeModeOf = (verge: IVergeConfig | null | undefined) =>
  verge ? (verge.theme_mode ?? 'system') : undefined
