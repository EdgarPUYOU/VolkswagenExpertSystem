// Small shared pieces used by several pages.
import type { ReactNode } from 'react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { cn } from '@/lib/utils'

export function PageHeader({ title, subtitle, children }: { title: string; subtitle?: string; children?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-3">
      <div>
        <h1 className="text-2xl font-semibold">{title}</h1>
        {subtitle && <p className="text-muted-foreground text-sm">{subtitle}</p>}
      </div>
      {children}
    </div>
  )
}

export function Kpi({ title, value, hint, tone }: { title: string; value: string; hint?: string; tone?: 'bad' | 'good' }) {
  return (
    <Card>
      <CardHeader>
        <CardDescription>{title}</CardDescription>
        <CardTitle className={cn('text-3xl tabular-nums', tone === 'bad' && 'text-destructive')}>{value}</CardTitle>
      </CardHeader>
      {hint && <CardContent className="text-muted-foreground text-sm">{hint}</CardContent>}
    </Card>
  )
}

export function Loading({ query }: { query: { isLoading: boolean; error: Error | null } }) {
  if (query.error) return <p className="text-destructive text-sm">{query.error.message}</p>
  if (query.isLoading) return <p className="text-muted-foreground text-sm">Loading…</p>
  return null
}

export const fieldClass =
  'border-input bg-background h-8 rounded-lg border px-2.5 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50'

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1 text-xs">
      <span className="text-muted-foreground">{label}</span>
      {children}
    </label>
  )
}
