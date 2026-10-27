import assert from 'node:assert/strict'
import test from 'node:test'

import {
  PALETTES,
  avatarBackground,
  hashSeed,
  initialsFor,
  paletteFor,
} from './avatar.js'

test('同一用户 id 恒定映射到同一配色', () => {
  const id = '0f3d2c1a-4b5e-6f70-8192-a3b4c5d6e7f8'
  assert.deepEqual(paletteFor(id), paletteFor(id))
  assert.equal(avatarBackground(id), avatarBackground(id))
})

test('不同用户 id 能覆盖到多组配色', () => {
  const seen = new Set()
  for (let index = 0; index < 200; index += 1) {
    seen.add(PALETTES.indexOf(paletteFor(`user-${index}`)))
  }
  assert.ok(seen.size >= 6, `配色覆盖不足：${seen.size}`)
})

test('哈希为空值时仍有确定结果', () => {
  assert.equal(hashSeed(null), hashSeed(''))
  assert.equal(hashSeed(undefined), hashSeed(''))
  assert.ok(PALETTES.includes(paletteFor('')))
})

test('中文显示名取首字', () => {
  assert.equal(initialsFor('张三', 'zhangsan'), '张')
  assert.equal(initialsFor('欧阳锋', 'ouyang'), '欧')
  assert.equal(initialsFor('', '李四'), '李')
})

test('拉丁显示名按分隔符取一到两个字母', () => {
  assert.equal(initialsFor('john.doe', 'john'), 'JD')
  assert.equal(initialsFor('Ada Lovelace', 'ada'), 'AL')
  assert.equal(initialsFor('admin', 'admin'), 'AD')
  assert.equal(initialsFor('a', 'a'), 'A')
})

test('混合显示名以首字符类型决定', () => {
  // 首字符是 CJK → 只取该字。
  assert.equal(initialsFor('张 San', 'zhang'), '张')
  // 首字符是拉丁 → 走两段规则。
  assert.equal(initialsFor('San 张', 'san'), 'S张')
})

test('空值回落固定符号', () => {
  assert.equal(initialsFor('', ''), '?')
  assert.equal(initialsFor(null, undefined), '?')
  assert.equal(initialsFor('   ', '   '), '?')
})

test('emoji 首字符不被截断', () => {
  assert.equal(initialsFor('🦊 fox', 'fox'), '🦊F')
})

test('配色只由种子决定，与显示名无关', () => {
  const id = 'stable-user-id'
  const before = paletteFor(id)
  // 显示名变化不影响 seed（组件层用 id 做 seed），这里直接验证函数契约。
  assert.deepEqual(paletteFor(id), before)
  assert.notEqual(avatarBackground(id).length, 0)
})
