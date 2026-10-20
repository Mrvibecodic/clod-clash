import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { awaitsCopy, providerImageView } from './provider-image.ts'

const REMOTE = 'https://example.com/icon.png'

describe('providerImageView', () => {
  it('never hands a remote link to the window while the copy is missing', () => {
    assert.equal(providerImageView(REMOTE, undefined), 'pending')
    assert.equal(providerImageView(REMOTE, 'pending'), 'pending')
  })

  it('leaves no placeholder for a picture the app will never fetch', () => {
    assert.equal(
      providerImageView('http://cdn.example.com/icon.png', 'never'),
      undefined,
    )
    assert.equal(awaitsCopy('never'), false)
    assert.equal(awaitsCopy({ ready: 'data:image/png;base64,AA' }), false)
    assert.equal(awaitsCopy('pending'), true)
    assert.equal(awaitsCopy(undefined), true)
  })

  it('shows the downloaded copy once it is ready', () => {
    assert.deepEqual(
      providerImageView(REMOTE, { ready: 'data:image/png;base64,AA' }),
      { src: 'data:image/png;base64,AA' },
    )
  })

  it('draws inline icons by itself', () => {
    assert.deepEqual(providerImageView(' data:image/png;base64,AA ', 'never'), {
      src: 'data:image/png;base64,AA',
    })
    assert.deepEqual(providerImageView('<svg/>', undefined), {
      src: 'data:image/svg+xml;charset=utf-8,%3Csvg%2F%3E',
    })
  })

  it('draws nothing without an icon', () => {
    assert.equal(providerImageView(undefined, 'pending'), undefined)
    assert.equal(providerImageView('icon.png', 'pending'), undefined)
  })

  it('the provider logo on Home comes only from the copy', () => {
    const source = readFileSync(
      new URL('../components/home/provider-header.tsx', import.meta.url),
      'utf8',
    )
    assert.match(source, /providerImageView\(profile\.logo, cachedLogo/)
    assert.doesNotMatch(source, /\?\? profile\.logo/)
  })
})
