export interface EditorReadFailure {
  reason: unknown
}

/** Файл не прочитался — сохранять нельзя: пустой редактор затёр бы его. */
export const editorSaveBlocked = (
  loading: boolean,
  failure: EditorReadFailure | null | undefined,
  dirty: boolean | undefined,
) => loading || Boolean(failure) || dirty === false
