import assert from 'node:assert/strict'
import test from 'node:test'

import {
  detectCredentialKind,
  expandTemplate,
  healthState,
  isPresetEntry,
  presetDisplayName,
  presetOf,
  requiredAuthScheme,
  unprobeableReason,
} from './providerEndpoint.js'

const PRESETS = [
  { key: 'openai', name: 'OpenAI', base_url_template: 'https://api.openai.com/v1', base_url_variables: [] },
  {
    key: 'azure',
    name: 'Azure',
    base_url_template: 'https://{resource}.openai.azure.com',
    base_url_variables: [{ name: 'resource', kind: 'host-label', required: true }],
  },
  {
    key: 'openai-compatible',
    name: 'OpenAI Compatible',
    base_url_template: '${BASE_URL}',
    base_url_variables: [{ name: 'BASE_URL', kind: 'full-url', required: true }],
  },
  { key: 'google-vertex', name: 'Vertex', base_url_template: 'https://{location}-aiplatform.googleapis.com' },
]

test('凭据内容识别：SA JSON 与 API Key', () => {
  assert.equal(detectCredentialKind('sk-abc'), 'api_key')
  assert.equal(detectCredentialKind('{"type":"service_account"}'), 'google_service_account')
  assert.equal(detectCredentialKind('{"type":"other"}'), 'api_key')
  assert.equal(detectCredentialKind('{not json'), 'api_key')
  assert.equal(detectCredentialKind(''), 'api_key')
  assert.equal(
    detectCredentialKind('{"access_key_id":"AKIA","secret_access_key":"s"}'),
    'aws_sigv4',
  )
  assert.equal(detectCredentialKind('{"access_key_id":"AKIA"}'), 'api_key')
})

test('凭据类型隐含的鉴权方式', () => {
  assert.equal(requiredAuthScheme('google_service_account'), 'google_service_account')
  assert.equal(requiredAuthScheme('aws_sigv4'), 'sigv4')
  assert.equal(requiredAuthScheme('api_key'), null)
})

test('模板展开：两种占位符与去空白', () => {
  assert.equal(expandTemplate('https://api.openai.com/v1', {}), 'https://api.openai.com/v1')
  assert.equal(
    expandTemplate('https://{resource}.openai.azure.com', { resource: ' my-resource ' }),
    'https://my-resource.openai.azure.com',
  )
  assert.equal(
    expandTemplate('${BASE_URL}', { BASE_URL: 'https://proxy.example.com/v1' }),
    'https://proxy.example.com/v1',
  )
  assert.equal(
    expandTemplate('https://{location}-aiplatform.googleapis.com', { location: 'us-east5' }),
    'https://us-east5-aiplatform.googleapis.com',
  )
})

test('预设反查：精确、单次剥离 -custom、不递归', () => {
  assert.equal(presetOf(PRESETS, 'azure')?.key, 'azure')
  assert.equal(presetOf(PRESETS, 'azure-custom')?.key, 'azure')
  assert.equal(presetOf(PRESETS, 'azure-custom-custom'), undefined)
  assert.equal(presetOf(PRESETS, 'my-proxy'), undefined)
  assert.equal(isPresetEntry(PRESETS, 'azure'), true)
  assert.equal(isPresetEntry(PRESETS, 'azure-custom'), false)
})

test('预设本地化名称', () => {
  const preset = { name: 'Alibaba Coding Plan', name_zh: '阿里云编程套餐' }
  assert.equal(presetDisplayName(preset, 'zh-CN'), '阿里云编程套餐')
  assert.equal(presetDisplayName(preset, 'en-US'), 'Alibaba Coding Plan')
  assert.equal(presetDisplayName({ name: 'OpenAI' }, 'zh-CN'), 'OpenAI')
})

test('不可探测原因：SA / 空模型 / 无入站格式', () => {
  assert.equal(
    unprobeableReason({ credential_kind: 'google_service_account', models: [{ name: 'm' }] }),
    'health.unprobeableServiceAccount',
  )
  assert.equal(unprobeableReason({ credential_kind: 'api_key', models: [] }), 'health.unprobeableNoModels')
  assert.equal(
    unprobeableReason({ credential_kind: 'api_key', models: [{ name: 'm' }] }),
    'health.unprobeableNoFormats',
  )
})

test('健康列四态优先级', () => {
  const probeable = { probeable: true, models: [{ name: 'm' }] }
  const notProbeable = { probeable: false, models: [{ name: 'm' }] }
  assert.equal(healthState(probeable, { state: 'retired' }), 'retired')
  assert.equal(healthState(probeable, { state: 'quarantined' }), 'cooldown')
  assert.equal(healthState(notProbeable, null), 'unprobeable')
  assert.equal(healthState(probeable, null), 'healthy')
})
