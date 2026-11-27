import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from 'recharts'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { api, eur, pct, shortName, signed, type MarginTrace } from '@/lib/api'

const config = { margin: { label: 'Margin %', color: 'var(--chart-1)' } } satisfies ChartConfig

type Comp = MarginTrace['components'][number]

const columns: ColumnDef<Comp>[] = [
  { accessorKey: 'component', header: 'Component' },
  { accessorKey: 'supplier', header: 'Supplier' },
  { accessorKey: 'currency', header: 'Currency' },
  { accessorKey: 'price_from', header: 'Unit price before', cell: (c) => eur.format(c.getValue<number>()) },
  { accessorKey: 'price_to', header: 'Unit price now', cell: (c) => eur.format(c.getValue<number>()) },
  { accessorKey: 'delta_eur_per_unit', header: 'Change per car', cell: (c) => `${signed(c.getValue<number>())} EUR` },
  {
    id: 'why',
    header: 'Why',
    enableSorting: false,
    accessorFn: (r) => r.drivers.length,
    cell: (c) => {
      const r = c.row.original
      return (
        <div className="flex flex-wrap gap-1">
          {r.drivers.filter((d) => Math.abs(d.delta_eur_per_unit) >= 0.05).map((d) => (
            <Badge key={d.index} variant="outline">
              {d.index} {signed(d.delta_eur_per_unit)} (weight {pct(d.weight * 100, 0)})
            </Badge>
          ))}
          {Math.abs(r.fx_effect_eur_per_unit) >= 0.05 && <Badge variant="secondary">{r.currency} exchange rate {signed(r.fx_effect_eur_per_unit)}</Badge>}
        </div>
      )
    },
  },
]

export default function Explain() {
  const { model = 'Golf' } = useParams()
  const navigate = useNavigate()
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const models = useQuery({ queryKey: ['models'], queryFn: api.models })
  const trace = useQuery({ queryKey: ['trace', model, from, to], queryFn: () => api.marginTrace(model, from || undefined, to || undefined) })
  const t = trace.data
  const paths = t?.components.flatMap((c) => c.path) ?? []

  return (
    <div className="space-y-6">
      <PageHeader title="Explain a margin" subtitle="Why did a model's margin move? Each cost change is traced through the knowledge graph: model, component, contract, price clause, index.">
        <div className="flex w-full flex-wrap items-end gap-3 sm:w-auto">
          <Field label="Model">
            <select className={fieldClass} value={model} onChange={(e) => navigate(`/explain/${e.target.value}`)}>
              {(models.data ?? []).map((m) => (
                <option key={m.model} value={shortName(m.model)}>{m.label.startsWith(m.brand) ? m.label : `${m.brand} ${m.label}`}</option>
              ))}
            </select>
          </Field>
          <Field label="From">
            <input className={fieldClass} type="month" value={from} onChange={(e) => setFrom(e.target.value)} />
          </Field>
          <Field label="To">
            <input className={fieldClass} type="month" value={to} onChange={(e) => setTo(e.target.value)} />
          </Field>
        </div>
      </PageHeader>
      <Loading query={trace} />
      {t && (
        <>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Kpi title={`Margin in ${t.from}`} value={pct(t.margin_from_pct, 2)} />
            <Kpi title={`Margin in ${t.to}`} value={pct(t.margin_to_pct, 2)} />
            <Kpi title="Change" value={`${signed(t.change_pp, 2)} points`} tone={t.change_pp < 0 ? 'bad' : 'good'} />
            <Kpi title="Material cost change per car" value={`${signed(t.material_change_eur_per_unit)} EUR`} hint={`on a price of ${eur.format(t.price)}`} />
          </div>
          <Card>
            <CardHeader>
              <CardTitle>{t.label}: margin by month</CardTitle>
              <CardDescription>Only material cost moves; price and other costs are held constant.</CardDescription>
            </CardHeader>
            <CardContent>
              <ChartContainer config={config} className="h-56 w-full">
                <LineChart data={t.series.map((s) => ({ month: s.month, margin: Number(s.margin_pct.toFixed(3)) }))}>
                  <CartesianGrid vertical={false} />
                  <XAxis dataKey="month" tickLine={false} axisLine={false} />
                  <YAxis domain={['auto', 'auto']} tickLine={false} axisLine={false} width={44} unit="%" />
                  <ChartTooltip content={<ChartTooltipContent />} />
                  <Line dataKey="margin" stroke="var(--color-margin)" strokeWidth={2} dot />
                </LineChart>
              </ChartContainer>
            </CardContent>
          </Card>
          <DataTable columns={columns} data={t.components} />
          <details className="text-sm">
            <summary className="cursor-pointer font-medium">Knowledge-graph paths ({paths.length})</summary>
            <ul className="text-muted-foreground mt-2 space-y-1 font-mono text-xs">
              {paths.map((p) => <li key={p}>{p}</li>)}
            </ul>
          </details>
        </>
      )}
    </div>
  )
}
