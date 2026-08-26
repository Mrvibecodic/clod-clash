import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { parseBannerText } from './banner-text.ts'

interface FixtureCase {
  name: string
  input: string
  fragments: [string, string | null][]
  visible: number
  limit: number
  truncated: string
}

const cases = JSON.parse(
  readFileSync(new URL('./banner-text.fixtures.json', import.meta.url), 'utf8'),
) as FixtureCase[]

const shape = (text: string) =>
  parseBannerText(text).map(
    ({ text: body, color }) => [body, color ?? null] as const,
  )

const visibleLength = (text: string) =>
  parseBannerText(text).reduce(
    (sum, fragment) => sum + Array.from(fragment.text).length,
    0,
  )

describe('фикстуры баннеров, общие с truncate_banner на Rust', () => {
  assert.ok(cases.length >= 5)

  for (const fixture of cases) {
    it(fixture.name, () => {
      assert.deepEqual(shape(fixture.input), fixture.fragments)
      assert.equal(visibleLength(fixture.input), fixture.visible)
      assert.equal(
        visibleLength(fixture.truncated),
        Math.min(fixture.visible, fixture.limit),
      )
    })
  }
})
