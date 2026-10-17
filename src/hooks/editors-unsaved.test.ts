import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { dismissUnlessDirty } from '../utils/dialog-dismiss.ts'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')

describe('окна с несохранённым', () => {
  it('несохранённое Esc и щелчок мимо окна не закрывают', () => {
    assert.equal(dismissUnlessDirty(true, 'close'), undefined)
    assert.equal(dismissUnlessDirty(false, 'close'), 'close')
    assert.equal(dismissUnlessDirty(undefined, 'close'), 'close')
  })

  it('правило одно: общее окно (Настройки, подписка) и редакторы подписки', () => {
    assert.match(
      source('../components/base/base-dialog.tsx'),
      /onClose=\{dismissUnlessDirty\(dirty \?\? Boolean\(changes\), onClose\)\}/,
    )
    assert.match(
      source('../components/profile/profile-viewer.tsx'),
      /dirty=\{!added && \(isDirty \|\| Boolean\(fileName\)\)\}/,
    )
    for (const path of [
      '../components/profile/rules-editor-viewer.tsx',
      '../components/profile/proxies-editor-viewer.tsx',
      '../components/profile/groups-editor-viewer.tsx',
      '../components/profile/editor-viewer.tsx',
    ]) {
      assert.match(
        source(path),
        /onClose=\{dirty \? undefined : (onClose|handleClose)\}/,
        path,
      )
    }
  })

  it('признак несохранённого у наглядного вида — от диска, у текста — от чистого текста', () => {
    const hook = source('./use-seq-document.ts')
    assert.match(hook, /visualization \? seqDirty : text !== cleanText/)
    assert.match(hook, /if \(!seqDirty\) setCleanText\(dumped\)/)
  })
})
