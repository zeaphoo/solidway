import assert from 'node:assert/strict'
import { test } from 'node:test'

import { SCORING_STRATEGIES, fieldsForStrategy } from './selectionDefaults.js'

test('切到综合加权时给出非零默认权重与探索率', () => {
  // 回归：此前切策略沿用上一个策略的 resolved（轮询的权重是 0），
  // 导致权重与探索率全部显示 0，保存即「回退轮询」。
  const fields = fieldsForStrategy('auto', {})
  assert.deepEqual(fields.weights, { uptime: '0.5', latency: '0.25', throughput: '0.25' })
  assert.equal(fields.thresholds.exploration_rate, '0.01')
  assert.equal(fields.thresholds.default_tokens_per_sec, '30')
})

test('非打分策略的权重为 0（但界面不展示）', () => {
  for (const strategy of ['round_robin', 'last_used']) {
    assert.equal(SCORING_STRATEGIES.has(strategy), false)
    const fields = fieldsForStrategy(strategy, {})
    assert.deepEqual(fields.weights, { uptime: '0', latency: '0', throughput: '0' })
    assert.equal(fields.thresholds.exploration_rate, '0')
  }
})

test('时延与吞吐策略各有一项主导权重', () => {
  assert.deepEqual(fieldsForStrategy('latency', {}).weights, {
    uptime: '0.1',
    latency: '0.9',
    throughput: '0',
  })
  assert.deepEqual(fieldsForStrategy('throughput', {}).weights, {
    uptime: '0.1',
    latency: '0',
    throughput: '0.9',
  })
})

test('已保存的覆盖项优先于策略默认值', () => {
  const fields = fieldsForStrategy('auto', {
    weights: { latency: 0.1 },
    thresholds: { exploration_rate: 0, default_tokens_per_sec: 12 },
  })
  assert.deepEqual(fields.weights, { uptime: '0.5', latency: '0.1', throughput: '0.25' })
  assert.equal(fields.thresholds.exploration_rate, '0')
  assert.equal(fields.thresholds.default_tokens_per_sec, '12')
})

test('未知策略回落到轮询默认值', () => {
  const fields = fieldsForStrategy('nope', {})
  assert.deepEqual(fields.weights, { uptime: '0', latency: '0', throughput: '0' })
})
