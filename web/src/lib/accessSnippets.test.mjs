import assert from 'node:assert/strict'
import { test } from 'node:test'

import { FORMATS, buildBaseUrls, formatsAndModels } from './accessSnippets.js'

test('buildBaseUrls：无配置时回落当前访问地址', () => {
  assert.deepEqual(buildBaseUrls([], 'https://gw.local:20975'), ['https://gw.local:20975/v1'])
})

test('buildBaseUrls：已带 /v1 不重复拼接', () => {
  assert.deepEqual(buildBaseUrls(['https://gw.example.com/v1'], 'http://x'), [
    'https://gw.example.com/v1',
  ])
})

test('buildBaseUrls：去尾斜杠、去空白、去重、保序', () => {
  assert.deepEqual(
    buildBaseUrls(
      [' https://a.example.com/ ', 'https://b.example.com/v1/', '', 'https://a.example.com'],
      'http://x',
    ),
    ['https://a.example.com/v1', 'https://b.example.com/v1'],
  )
})

test('buildBaseUrls：空值全部过滤后退回 fallback', () => {
  assert.deepEqual(buildBaseUrls(['', '   '], 'https://gw.local/'), ['https://gw.local/v1'])
  assert.deepEqual(buildBaseUrls(null, null), [])
})

test('formatsAndModels：只统计 active 凭据，并按固定顺序返回格式', () => {
  const { availableFormats, modelsByFormat } = formatsAndModels([
    {
      status: 'active',
      api_formats: ['anthropic_messages', 'chat_completions', 'native'],
      models: [{ name: 'claude-sonnet-4' }, { name: 'claude-haiku' }],
    },
    {
      status: 'disabled',
      api_formats: ['responses'],
      models: [{ name: 'gpt-5' }],
    },
    {
      status: 'active',
      api_formats: ['chat_completions'],
      models: [{ name: 'gpt-5' }, { name: '' }],
    },
  ])

  assert.deepEqual(availableFormats, ['chat_completions', 'anthropic_messages'])
  assert.deepEqual(modelsByFormat.chat_completions, ['claude-haiku', 'claude-sonnet-4', 'gpt-5'])
  assert.deepEqual(modelsByFormat.anthropic_messages, ['claude-haiku', 'claude-sonnet-4'])
  assert.deepEqual(modelsByFormat.responses, [])
})

test('formatsAndModels：空输入返回全空结构', () => {
  const { availableFormats, modelsByFormat } = formatsAndModels(undefined)
  assert.deepEqual(availableFormats, [])
  assert.deepEqual(Object.keys(modelsByFormat), FORMATS)
  for (const format of FORMATS) assert.deepEqual(modelsByFormat[format], [])
})
