export interface EditorReadFailure {
  reason: unknown
}

export const editorSaveBlocked = (
  loading: boolean,
  failure: EditorReadFailure | null | undefined,
  dirty: boolean | undefined,
) => loading || Boolean(failure) || dirty === false
