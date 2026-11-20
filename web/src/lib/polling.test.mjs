import assert from 'node:assert/strict'
import { test } from 'node:test'

import { pollEvery, pollUntil, shouldKeepPolling } from './polling.js'

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** 跑一次有界轮询，等 `onDone` 后返回是否达成（避免用固定 sleep 猜时序）。 */
function runPoll(task, options = {}) {
  return new Promise((resolve) => {
    pollUntil(task, { ...options, onDone: resolve })
  })
}

test('4xx 停表，5xx / 网络错误 / 无错误继续', () => {
  for (const status of [400, 401, 403, 404, 422, 499]) {
    assert.equal(shouldKeepPolling({ status }), false, `status ${status} 应停表`)
  }
  for (const error of [undefined, null, { status: 0 }, { status: 500 }, { status: 503 }, new Error('x')]) {
    assert.equal(shouldKeepPolling(error), true, `${String(error)} 应继续`)
  }
})

test('pollEvery 在 4xx 时返回 false，否则返回原间隔', () => {
  const interval = pollEvery(15_000)
  assert.equal(interval({ state: { error: null } }), 15_000)
  assert.equal(interval({ state: { error: { status: 500 } } }), 15_000)
  assert.equal(interval({ state: { error: { status: 401 } } }), false)
})

test('pollUntil 达成后停止', async () => {
  let calls = 0
  const done = await runPoll(
    () => {
      calls += 1
      return calls === 3
    },
    { intervalMs: 1, attempts: 10 },
  )
  assert.equal(done, true)
  assert.equal(calls, 3)
})

test('pollUntil 用尽次数后返回 false 且不再请求', async () => {
  let calls = 0
  const done = await runPoll(
    () => {
      calls += 1
      return false
    },
    { intervalMs: 1, attempts: 3 },
  )
  assert.equal(done, false)
  assert.equal(calls, 3)
})

test('pollUntil 取消后不再请求也不回调', async () => {
  let calls = 0
  let doneCalls = 0
  const cancel = pollUntil(
    () => {
      calls += 1
      return false
    },
    { intervalMs: 2, onDone: () => (doneCalls += 1) },
  )
  cancel()
  await sleep(20)
  assert.equal(calls, 0)
  assert.equal(doneCalls, 0)
})

test('pollUntil 任务慢于间隔时不重叠', async () => {
  let active = 0
  let maxActive = 0
  let calls = 0
  const done = await runPoll(
    async () => {
      calls += 1
      active += 1
      maxActive = Math.max(maxActive, active)
      await sleep(10)
      active -= 1
      return calls >= 2
    },
    { intervalMs: 1, attempts: 5 },
  )
  assert.equal(done, true)
  assert.equal(calls, 2)
  assert.equal(maxActive, 1)
})

test('pollUntil 任务抛错按未达成分支继续', async () => {
  let calls = 0
  const done = await runPoll(
    () => {
      calls += 1
      if (calls < 2) throw new Error('boom')
      return true
    },
    { intervalMs: 1, attempts: 5 },
  )
  assert.equal(done, true)
  assert.equal(calls, 2)
})
