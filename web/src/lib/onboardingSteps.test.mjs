import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  GROUPS,
  OPS_STEPS,
  SETUP_STEPS,
  buildContext,
  evaluateProgress,
  onboardingSteps,
  shouldAutoEnter,
} from './onboardingSteps.js'

const route = { id: 'r1', name: 'prod' }
const emptyCtx = buildContext()

function ctxWith(overrides) {
  return buildContext({ routes: [route], ...overrides })
}

test('步骤分组符合设计：接入 4 步 + 运维 3 步', () => {
  assert.equal(SETUP_STEPS.length, 4)
  assert.equal(OPS_STEPS.length, 3)
  assert.deepEqual([...new Set(SETUP_STEPS.map((s) => s.group))], ['setup'])
  assert.deepEqual([...new Set(OPS_STEPS.map((s) => s.group))], ['ops'])
  assert.deepEqual(GROUPS, ['setup', 'ops'])
})

test('按角色拼装：普通用户只有接入组', () => {
  const user = onboardingSteps('user')
  const admin = onboardingSteps('admin')
  assert.equal(user.length, 4)
  assert.equal(admin.length, 7)
  assert.equal(admin.filter((s) => s.group === 'ops').length, 3)
  assert.equal(user.filter((s) => s.group === 'ops').length, 0)
})

test('空上下文：全部未完成且不抛错', () => {
  const progress = evaluateProgress(onboardingSteps('user'), emptyCtx)
  assert.equal(progress.done, 0)
  assert.equal(progress.total, 4)
  assert.equal(progress.percent, 0)
  assert.equal(progress.complete, false)
  assert.equal(progress.nextStepId, 'route')
})

test('逐步推进：路由 → 凭据 → Key → 首调', () => {
  const steps = onboardingSteps('user')

  let progress = evaluateProgress(steps, ctxWith({}))
  assert.equal(progress.done, 1)
  assert.equal(progress.nextStepId, 'provider')

  progress = evaluateProgress(steps, ctxWith({ providerKeys: [{ id: 'k1' }] }))
  assert.equal(progress.done, 2)
  assert.equal(progress.nextStepId, 'apiKey')

  progress = evaluateProgress(steps, ctxWith({ providerKeys: [{ id: 'k1' }], apiKeys: [{ id: 'a1' }] }))
  assert.equal(progress.done, 3)
  assert.equal(progress.nextStepId, 'firstCall')

  progress = evaluateProgress(
    steps,
    ctxWith({ providerKeys: [{ id: 'k1' }], apiKeys: [{ id: 'a1' }], usage: { requests: 1 } }),
  )
  assert.equal(progress.done, 4)
  assert.equal(progress.complete, true)
  assert.equal(progress.percent, 100)
  assert.equal(progress.nextStepId, null)
})

test('删除对象后进度回退（完成状态只认实时数据）', () => {
  const steps = onboardingSteps('user')
  const full = ctxWith({
    providerKeys: [{ id: 'k1' }],
    apiKeys: [{ id: 'a1' }],
    usage: { requests: 3 },
  })
  assert.equal(evaluateProgress(steps, full).complete, true)

  const afterDelete = ctxWith({ providerKeys: [{ id: 'k1' }], apiKeys: [], usage: { requests: 3 } })
  const progress = evaluateProgress(steps, afterDelete)
  assert.equal(progress.done, 3)
  assert.equal(progress.complete, false)
  assert.equal(progress.nextStepId, 'apiKey')
})

test('admin 运维组：对外地址 / 员工账号 / 告警', () => {
  const steps = onboardingSteps('admin')
  const ctx = buildContext({
    routes: [route],
    providerKeys: [{ id: 'k1' }],
    apiKeys: [{ id: 'a1' }],
    usage: { requests: 1 },
    access: { public_origins: ['https://gw.example.com'] },
    users: [{ id: 'u1', role: 'admin' }, { id: 'u2', role: 'user' }],
    settings: { alert: { webhook_url: 'https://hooks.example.com/x' } },
  })
  const progress = evaluateProgress(steps, ctx)
  assert.equal(progress.total, 7)
  assert.equal(progress.complete, true)
})

test('admin 运维组：只有 admin 自己时员工账号未完成', () => {
  const steps = onboardingSteps('admin').filter((s) => s.id === 'staffAccount')
  const progress = evaluateProgress(steps, buildContext({ users: [{ id: 'u1', role: 'admin' }] }))
  assert.equal(progress.done, 0)
})

test('样板路由与动作链接：没有路由时回落到总览', () => {
  const steps = onboardingSteps('user')
  const withoutRoute = evaluateProgress(steps, emptyCtx)
  assert.deepEqual(withoutRoute.rows[1].action, { kind: 'link', to: '/routes' })
  assert.equal(withoutRoute.rows[1].hint, null)

  const withRoute = evaluateProgress(steps, ctxWith({}))
  assert.deepEqual(withRoute.rows[1].action, { kind: 'link', to: '/routes/r1?new=provider' })
  assert.deepEqual(withRoute.rows[1].hint, {
    key: 'onboarding.steps.provider.sample',
    params: { name: 'prod' },
  })
  assert.deepEqual(withRoute.rows[0].action, { kind: 'dialog', dialog: 'route' })
  assert.deepEqual(withRoute.rows[2].action, { kind: 'link', to: '/api-keys?new=1' })
})

test('pending 时不给出百分比（避免 0% 误导）', () => {
  const progress = evaluateProgress(onboardingSteps('user'), emptyCtx, true)
  assert.equal(progress.percent, 0)
  assert.equal(progress.total, 4)
})

test('自动进入判定：无路由且未标记才进入', () => {
  assert.equal(shouldAutoEnter({ routeCount: 0, dismissed: false }), true)
  assert.equal(shouldAutoEnter({ routeCount: 0, dismissed: true }), false)
  assert.equal(shouldAutoEnter({ routeCount: 2, dismissed: false }), false)
  assert.equal(shouldAutoEnter({}), true)
})
