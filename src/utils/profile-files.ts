import type { EditorReadFailure } from './editor-save.ts'

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
