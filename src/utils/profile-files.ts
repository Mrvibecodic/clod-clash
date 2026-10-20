import type { EditorReadFailure } from './editor-save.ts'

/**
 * Читает файлы подписки каждый сам по себе: не прочитавшийся отдаёт пустой
 * текст и причину, остальные доходят до окна.
 */
export const readEachFile = async (
  uids: string[],
  read: (uid: string) => Promise<string>,
): Promise<{ texts: string[]; failure: EditorReadFailure | null }> => {
  const results = await Promise.allSettled(uids.map((uid) => read(uid)))
  const rejected = results.find((result) => result.status === 'rejected')
  return {
    texts: results.map((result) =>
      result.status === 'fulfilled' ? result.value : '',
    ),
    failure: rejected ? { reason: rejected.reason } : null,
  }
}
