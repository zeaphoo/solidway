/**
 * 快速开始的步骤定义与进度推导。
 *
 * 这里是唯一的「步骤事实来源」：页面只负责渲染 `evaluateProgress` 的结果，
 * 不自行判断完成状态、不自行解释接口数据。全部为纯函数，便于单测。
 */

/** 分组顺序：接入（所有角色）→ 运维（仅 admin）。 */
export const GROUPS = ['setup', 'ops']

/** 接入：所有角色都需要的 4 步。 */
export const SETUP_STEPS = [
  {
    id: 'route',
    group: 'setup',
    titleKey: 'onboarding.steps.route.title',
    descKey: 'onboarding.steps.route.desc',
    helpAnchor: 'task-create-route',
    action: () => ({ kind: 'dialog', dialog: 'route' }),
    isDone: (ctx) => ctx.routes.length > 0,
  },
  {
    id: 'provider',
    group: 'setup',
    titleKey: 'onboarding.steps.provider.title',
    descKey: 'onboarding.steps.provider.desc',
    helpAnchor: 'task-add-provider',
    action: (ctx) =>
      ctx.sampleRoute
        ? { kind: 'link', to: `/routes/${ctx.sampleRoute.id}?new=provider` }
        : { kind: 'link', to: '/routes' },
    hint: (ctx) =>
      ctx.sampleRoute
        ? { key: 'onboarding.steps.provider.sample', params: { name: ctx.sampleRoute.name } }
        : null,
    isDone: (ctx) => ctx.providerKeys.length > 0,
  },
  {
    id: 'apiKey',
    group: 'setup',
    titleKey: 'onboarding.steps.apiKey.title',
    descKey: 'onboarding.steps.apiKey.desc',
    helpAnchor: 'task-create-key',
    action: () => ({ kind: 'link', to: '/api-keys?new=1' }),
    isDone: (ctx) => ctx.apiKeys.length > 0,
  },
  {
    id: 'firstCall',
    group: 'setup',
    titleKey: 'onboarding.steps.firstCall.title',
    descKey: 'onboarding.steps.firstCall.desc',
    helpAnchor: 'snippets',
    action: (ctx) =>
      ctx.sampleRoute
        ? { kind: 'link', to: `/routes/${ctx.sampleRoute.id}` }
        : { kind: 'link', to: '/help#snippets' },
    // 用量里有记录 = 真的调通了；比「点了复制」可信。
    isDone: (ctx) => Number(ctx.usage?.requests ?? 0) > 0,
  },
]

/** 运维：仅 admin 需要的 3 步，可稍后完成。 */
export const OPS_STEPS = [
  {
    id: 'publicOrigin',
    group: 'ops',
    titleKey: 'onboarding.steps.publicOrigin.title',
    descKey: 'onboarding.steps.publicOrigin.desc',
    helpAnchor: 'diagnostics',
    action: () => ({ kind: 'link', to: '/system' }),
    isDone: (ctx) => (ctx.access?.public_origins ?? []).filter(Boolean).length > 0,
  },
  {
    id: 'staffAccount',
    group: 'ops',
    titleKey: 'onboarding.steps.staffAccount.title',
    descKey: 'onboarding.steps.staffAccount.desc',
    helpAnchor: 'faq',
    action: () => ({ kind: 'link', to: '/users' }),
    isDone: (ctx) => ctx.users.some((user) => user?.role !== 'admin'),
  },
  {
    id: 'alertWebhook',
    group: 'ops',
    titleKey: 'onboarding.steps.alertWebhook.title',
    descKey: 'onboarding.steps.alertWebhook.desc',
    helpAnchor: 'trouble-401',
    action: () => ({ kind: 'link', to: '/system' }),
    isDone: (ctx) => Boolean(ctx.settings?.alert?.webhook_url),
  },
]

/** 按角色拼装步骤：普通用户只有接入组，admin 额外拿到运维组。 */
export function onboardingSteps(role) {
  return role === 'admin' ? [...SETUP_STEPS, ...OPS_STEPS] : [...SETUP_STEPS]
}

/**
 * 归一化上下文，保证判定函数拿到稳定形状（缺数据的步骤判为未完成）。
 * `sampleRoute` = 最近更新的一条路由，用于「有没有上游凭据」这类按路由的查询。
 */
export function buildContext({
  routes,
  apiKeys,
  providerKeys,
  usage,
  access,
  users,
  settings,
} = {}) {
  const routeList = routes ?? []
  return {
    routes: routeList,
    sampleRoute: routeList[0] ?? null,
    apiKeys: apiKeys ?? [],
    providerKeys: providerKeys ?? [],
    usage: usage ?? {},
    access: access ?? {},
    users: users ?? [],
    settings: settings ?? {},
  }
}

function safe(fn, ctx) {
  try {
    return Boolean(fn(ctx))
  } catch {
    return false
  }
}

/**
 * 推导进度：返回逐行状态、已完成数、下一步与百分比。
 *
 * `pending` 为真表示数据还在加载（进度不应展示成 0%）。
 */
export function evaluateProgress(steps, ctx, pending = false) {
  const rows = (steps ?? []).map((step) => ({
    ...step,
    done: safe(step.isDone, ctx),
    action: step.action?.(ctx) ?? null,
    hint: step.hint?.(ctx) ?? null,
  }))
  const done = rows.filter((row) => row.done).length
  const total = rows.length
  const next = rows.find((row) => !row.done) ?? null
  return {
    rows,
    done,
    total,
    percent: total && !pending ? Math.round((done / total) * 100) : 0,
    nextStepId: next?.id ?? null,
    complete: total > 0 && done === total,
  }
}

/** 是否应自动进入快速开始：无路由且本机未标记跳过 / 完成。 */
export function shouldAutoEnter({ routeCount, dismissed } = {}) {
  return Number(routeCount ?? 0) === 0 && !dismissed
}
