import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { providerImageView } from './provider-image.ts'

const REMOTE = 'https://example.com/icon.png'

describe('providerImageView', () => {
  it('never hands a remote link to the window while the copy is missing', () => {
    assert.equal(providerImageView(REMOTE, ''), 'pending')
    assert.equal(
      providerImageView('http://192.168.1.1/icon.png', ''),
      'pending',
    )
  })

  it('shows the downloaded copy once it is ready', () => {
    assert.deepEqual(providerImageView(REMOTE, 'asset://icon.png'), {
      src: 'asset://icon.png',
    })
  })

  it('draws inline icons by itself', () => {
    assert.deepEqual(providerImageView(' data:image/png;base64,AA ', ''), {
      src: 'data:image/png;base64,AA',
    })
    assert.deepEqual(providerImageView('<svg/>', ''), {
      src: 'data:image/svg+xml;charset=utf-8,%3Csvg%2F%3E',
    })
  })

  it('draws nothing without an icon', () => {
    assert.equal(providerImageView(undefined, ''), undefined)
    assert.equal(providerImageView('icon.png', ''), undefined)
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
