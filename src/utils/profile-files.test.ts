import assert from 'node:assert/strict'
import { test } from 'node:test'

import { readEachFile } from './profile-files.ts'

const disk: Record<string, string> = {
  profile: 'proxy-groups: []',
  merge: 'rule-providers: {}',
}
const read = async (uid: string) => {
  if (uid in disk) return disk[uid]
  throw new Error('access denied')
}

test('один файл не прочитался — остальные всё равно доходят, причина сохраняется', async () => {
  const { texts, failure } = await readEachFile(
    ['profile', 'groups', 'merge'],
    read,
  )
  assert.deepEqual(texts, ['proxy-groups: []', '', 'rule-providers: {}'])
  assert.equal((failure?.reason as Error).message, 'access denied')
})

test('всё прочиталось — причины нет', async () => {
  const { texts, failure } = await readEachFile(['profile', 'merge'], read)
  assert.deepEqual(texts, ['proxy-groups: []', 'rule-providers: {}'])
  assert.equal(failure, null)
})
