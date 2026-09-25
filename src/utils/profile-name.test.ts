import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { profileDisplayName } from './profile-name.ts'

describe('имя подписки на экране', () => {
  it('без ручного имени — имя от панели', () => {
    assert.equal(profileDisplayName({ name: 'Панель' }), 'Панель')
  })

  it('ручное имя идёт первым, имя от панели — в скобках', () => {
    assert.equal(
      profileDisplayName({
        name: 'Панель',
        custom_name: 'Работа',
        name_from_panel: true,
      }),
      'Работа (Панель)',
    )
  })

  it('совпадающие имена показываются одним', () => {
    assert.equal(
      profileDisplayName({
        name: 'Работа',
        custom_name: 'Работа',
        name_from_panel: true,
      }),
      'Работа',
    )
  })

  it('пустое ручное имя не считается', () => {
    assert.equal(
      profileDisplayName({ name: 'Панель', custom_name: '  ' }),
      'Панель',
    )
  })

  it('ручное имя без имени от панели показывается как есть', () => {
    assert.equal(profileDisplayName({ custom_name: 'Работа' }), 'Работа')
  })

  it('имя из адреса, а не от панели, рядом со своим не показывается', () => {
    assert.equal(
      profileDisplayName({ name: 'AbCdEf123', custom_name: 'Работа' }),
      'Работа',
    )
  })
})
