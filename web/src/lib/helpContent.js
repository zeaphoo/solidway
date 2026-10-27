/**
 * 帮助中心的结构事实：分区、条目、锚点与可选跳转。
 *
 * 只放结构与 key，不放文案——正文在 `locales/help/<lang>.json`，
 * 分区的 i18n key 形状为 `help.sections.<sectionId>.title` /
 * `help.items.<itemId>.{title,body,keywords,points}`。
 */

export const HELP_SECTIONS = [
  {
    id: 'concepts',
    icon: 'book',
    titleKey: 'help.sections.concepts.title',
    descKey: 'help.sections.concepts.desc',
    items: [
      { id: 'concept-route', to: '/routes' },
      { id: 'concept-provider-key' },
      { id: 'concept-api-key', to: '/api-keys' },
      { id: 'concept-quota-domain' },
    ],
  },
  {
    id: 'tasks',
    icon: 'list',
    titleKey: 'help.sections.tasks.title',
    descKey: 'help.sections.tasks.desc',
    items: [
      { id: 'task-create-route', to: '/routes' },
      { id: 'task-add-provider' },
      { id: 'task-create-key', to: '/api-keys?new=1' },
      { id: 'task-rotate-key', to: '/api-keys' },
      { id: 'task-disable-route' },
    ],
  },
  {
    id: 'snippets',
    icon: 'code',
    titleKey: 'help.sections.snippets.title',
    descKey: 'help.sections.snippets.desc',
    kind: 'snippets',
    items: [],
  },
  {
    id: 'troubleshoot',
    icon: 'wrench',
    titleKey: 'help.sections.troubleshoot.title',
    descKey: 'help.sections.troubleshoot.desc',
    items: [
      { id: 'trouble-401' },
      { id: 'trouble-403' },
      { id: 'trouble-cooldown', to: '/routes' },
      { id: 'trouble-no-model' },
      { id: 'trouble-tls', to: '/system' },
    ],
  },
  {
    id: 'diagnostics',
    icon: 'activity',
    titleKey: 'help.sections.diagnostics.title',
    descKey: 'help.sections.diagnostics.desc',
    kind: 'diagnostics',
    items: [],
  },
  {
    id: 'faq',
    icon: 'help',
    titleKey: 'help.sections.faq.title',
    descKey: 'help.sections.faq.desc',
    items: [
      { id: 'faq-switch', to: '/routes' },
      { id: 'faq-tls', to: '/system' },
      { id: 'faq-credentials' },
      { id: 'faq-quota' },
      { id: 'faq-scripts' },
      { id: 'faq-delete' },
    ],
  },
]

/** 全站锚点索引：分区 id 与条目 id 都能定位到所属分区。 */
const ANCHORS = new Map()
for (const section of HELP_SECTIONS) {
  ANCHORS.set(section.id, { section, item: null })
  for (const item of section.items) ANCHORS.set(item.id, { section, item })
}

/** 由锚点找分区与条目；未知锚点返回 `null`。 */
export function resolveAnchor(anchor) {
  if (!anchor) return null
  return ANCHORS.get(String(anchor).replace(/^#/, '')) ?? null
}

function lower(value) {
  return String(value ?? '').toLowerCase()
}

function itemMatches(item, sectionHit, keyword, t) {
  if (sectionHit) return true
  const haystack = [
    t(`help.items.${item.id}.title`),
    t(`help.items.${item.id}.body`),
    t(`help.items.${item.id}.keywords`),
  ]
    .join(' ')
    .toLowerCase()
  return haystack.includes(keyword)
}

/**
 * 按关键字过滤分区与条目（前端过滤，无索引）。
 *
 * `t` 传 i18n 的翻译函数，保证关键词跟随当前语言。返回每项
 * `{ section, items }`；无命中返回空数组。关键字为空时返回全部分区。
 */
export function filterSections(sections, query, t) {
  const keyword = lower(query).trim()
  if (!keyword) return (sections ?? []).map((section) => ({ section, items: section.items }))

  const result = []
  for (const section of sections ?? []) {
    const sectionHit = [t(section.titleKey), t(section.descKey), t(`help.sections.${section.id}.keywords`)]
      .join(' ')
      .toLowerCase()
      .includes(keyword)
    const items = section.items.filter((item) => itemMatches(item, sectionHit, keyword, t))
    if (sectionHit || items.length) result.push({ section, items })
  }
  return result
}
