/**
 * Solidway 品牌 mark —— 与 solidway_site/src/components/logo.ts 同源。
 *
 * 概念：实心块中开出一条通道，请求位于通道内；16px 下仍可辨识。
 * 使用 currentColor 绘制，颜色默认取 `--brand-mark`（按主题切换：
 * 深色用品牌绿 `#00e5a0`，浅色提深到 `#059669` 以保证白底对比度）。
 *
 * 形态约定：页面内的 logo 不自带底色（底色由页面给）；自带底色的深色
 * tile 版本只用于「图标位」（favicon / 应用图标 / 托盘），见 `web/public/favicon.svg`。
 */

const MARK_ARCS = [
  'M27 13v-2.5A4.5 4.5 0 0 0 22.5 6h-13A4.5 4.5 0 0 0 5 10.5V13',
  'M5 19v2.5A4.5 4.5 0 0 0 9.5 26h13a4.5 4.5 0 0 0 4.5-4.5V19',
]

const MARK_NODE = { cx: 16, cy: 16, r: 3 }
const MARK_STROKE_WIDTH = 2.6

export default function Logo({
  size = 26,
  withWordmark = false,
  markColor = 'var(--brand-mark)',
  wordClassName = 'text-foreground',
  className = '',
  label = 'Solidway',
}) {
  return (
    <span className={`inline-flex items-center gap-2.5 ${className}`}>
      <svg
        width={size}
        height={size}
        viewBox="0 0 32 32"
        fill="none"
        aria-hidden="true"
        focusable="false"
        className="shrink-0"
        style={{ color: markColor }}
      >
        {MARK_ARCS.map((d) => (
          <path
            key={d}
            d={d}
            stroke="currentColor"
            strokeWidth={MARK_STROKE_WIDTH}
            strokeLinecap="round"
          />
        ))}
        <circle cx={MARK_NODE.cx} cy={MARK_NODE.cy} r={MARK_NODE.r} fill="currentColor" />
      </svg>
      {withWordmark ? (
        <span className={`text-[15px] font-semibold tracking-[-0.022em] ${wordClassName}`}>
          {label}
        </span>
      ) : null}
    </span>
  )
}
