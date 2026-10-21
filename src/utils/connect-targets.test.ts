import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { connectTargets } from './connect-targets.ts'

type Pair = [boolean, boolean]
interface Case {
  verge: [boolean | null, boolean | null]
  profile: [string | null, boolean]
  targets: Pair
  locked: Pair | null
}

// Примеры общие с бэкендом (src-tauri/src/feat/connect.rs): окно, трей и
// горячие клавиши знают о замке одно и то же.
describe('способ подключения', () => {
  it('как в бэкенде', () => {
    const cases = JSON.parse(
      readFileSync(
        new URL('./connect-targets-cases.json', import.meta.url),
        'utf8',
      ),
    ) as Case[]
    assert.ok(cases.length > 0)
    for (const { verge, profile, targets, locked } of cases) {
      const got = connectTargets({
        connectSystemProxy: verge[0],
        connectTunMode: verge[1],
        connectMode: profile[0],
        lockMode: profile[1],
      })
      const label = JSON.stringify({ verge, profile })
      assert.deepEqual([got.targetSys, got.targetTun], targets, label)
      assert.equal(got.targetsLocked, locked !== null, label)
    }
  })
})
