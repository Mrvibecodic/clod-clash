import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { profileEditPatch } from './profile-edit.ts'

const opened: IProfileItem = {
  uid: 'a',
  type: 'remote',
  name: 'Панель',
  desc: 'описание',
  url: 'https://old.example/sub',
  updated: 100,
  option: { update_interval: 720, with_proxy: false, chan_pin: 'pin-old' },
}

describe('правка карточки подписки', () => {
  it('нетронутая карточка не отправляет ничего', () => {
    assert.deepEqual(profileEditPatch(opened, { ...opened }, opened.option), {})
  })

  it('нетронутое имя не уходит как выбор человека', () => {
    const patch = profileEditPatch(opened, { ...opened, group: 'Работа' })
    assert.deepEqual(patch, { group: 'Работа' })
  })

  it('поля, которых форма не правит, не возвращаются из снимка', () => {
    const patch = profileEditPatch(opened, { ...opened, desc: 'новое' })
    assert.equal('url' in patch, false)
    assert.equal('updated' in patch, false)
    assert.equal('option' in patch, false)
  })

  it('изменённая настройка ложится поверх свежей записи, а не снимка', () => {
    const latest = { ...opened.option, chan_pin: 'pin-new', merge: 'm1' }
    const patch = profileEditPatch(
      opened,
      { ...opened, option: { ...opened.option, with_proxy: true } },
      latest,
    )
    assert.deepEqual(patch.option, {
      update_interval: 720,
      with_proxy: true,
      chan_pin: 'pin-new',
      merge: 'm1',
    })
  })

  it('очищенная настройка уходит пустой, чтобы бэкенд её снял', () => {
    const withAgent = { ...opened, option: { user_agent: 'ua' } }
    const patch = profileEditPatch(
      withAgent,
      { ...withAgent, option: { user_agent: undefined } },
      withAgent.option,
    )
    assert.equal(patch.option?.user_agent, undefined)
    assert.equal(JSON.stringify(patch.option), '{}')
  })

  it('нулевое, пустое и не заданное значение настройки — одно и то же', () => {
    const fromHeaders: IProfileItem = {
      ...opened,
      option: { update_interval: 0, timeout_seconds: 0, user_agent: '' },
    }
    const patch = profileEditPatch(fromHeaders, {
      ...fromHeaders,
      option: {},
    })
    assert.deepEqual(patch, {})
  })

  it('выключенное автообновление отличается от не заданного', () => {
    const patch = profileEditPatch(opened, {
      ...opened,
      option: { ...opened.option, allow_auto_update: false },
    })
    assert.equal(patch.option?.allow_auto_update, false)
  })

  it('стёртое имя не отправляется: вернуть его может только панель', () => {
    assert.deepEqual(
      profileEditPatch(opened, { ...opened, name: undefined }),
      {},
    )
  })

  it('выключенный и не заданный переключатель — одно и то же', () => {
    const selfProxied: IProfileItem = {
      ...opened,
      option: { self_proxy: true },
    }
    const patch = profileEditPatch(selfProxied, {
      ...selfProxied,
      option: { self_proxy: true, with_proxy: false },
    })
    assert.deepEqual(patch, {})
  })

  it('пустая строка и отсутствие поля — одно и то же', () => {
    const bare: IProfileItem = { uid: 'b', type: 'local', name: 'x.yaml' }
    assert.deepEqual(profileEditPatch(bare, { ...bare, desc: '', url: '' }), {})
  })
})
