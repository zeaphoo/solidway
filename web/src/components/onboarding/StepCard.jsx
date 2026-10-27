import { ArrowRightIcon, CheckIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/**
 * 快速开始的单步卡片。
 *
 * 三种状态：已完成（压缩为一行）、下一步（高亮 + 主按钮）、未完成（次要按钮）。
 * 描述只在未完成或作为下一步时展示，已完成步骤只留标题与勾选。
 */
export default function StepCard({ index, step, isNext, onAction }) {
  const { t } = useTranslation()
  const showBody = !step.done || isNext

  return (
    <div
      className={cn(
        'rounded-lg border p-4 transition-colors',
        isNext && 'border-primary/40 bg-muted/40',
        step.done && 'border-dashed',
      )}
    >
      <div className="flex items-start gap-3">
        <span
          className={cn(
            'mt-0.5 grid size-6 shrink-0 place-items-center rounded-full border text-xs font-medium tabular-nums',
            step.done
              ? 'border-transparent bg-muted text-muted-foreground'
              : isNext
                ? 'border-primary text-foreground'
                : 'border-border text-muted-foreground',
          )}
          aria-hidden="true"
        >
          {step.done ? <CheckIcon className="size-3.5" /> : index + 1}
        </span>

        <div className="min-w-0 flex-1 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3
              className={cn(
                'text-sm font-medium',
                step.done && 'text-muted-foreground',
              )}
            >
              {t(step.titleKey)}
            </h3>
            {step.done ? (
              <span className="text-[11px] text-muted-foreground">{t('onboarding.done')}</span>
            ) : null}
          </div>

          {showBody ? (
            <p className="text-sm text-muted-foreground">{t(step.descKey)}</p>
          ) : null}

          {showBody && step.hint ? (
            <p className="text-xs text-muted-foreground">{t(step.hint.key, step.hint.params)}</p>
          ) : null}

          {showBody && step.helpAnchor ? (
            <Link
              to={`/help#${step.helpAnchor}`}
              className="inline-flex items-center gap-1 text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
            >
              {t('onboarding.detail')}
              <ArrowRightIcon className="size-3" />
            </Link>
          ) : null}
        </div>

        {step.action ? (
          <Button
            size="sm"
            variant={isNext && !step.done ? 'default' : 'outline'}
            className="shrink-0"
            onClick={() => onAction(step)}
          >
            {t(`onboarding.actions.${step.id}`)}
          </Button>
        ) : null}
      </div>
    </div>
  )
}
