import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useState } from 'react'
import { CartesianGrid, Line, LineChart, XAxis, YAxis } from 'recharts'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { api, eurCompact, pct, signed, type CurrencyExposure, type Sensitivity, type Series } from '@/lib/api'

const config = { value: { label: 'Value', color: 'var(--chart-1)' } } satisfies ChartConfig

function SeriesCard({ s }: { s: Series }) {
  return (
    <Card>
      <CardHeader>
        <CardDescription>{s.label}</CardDescription>
        <CardTitle className="flex items-baseline gap-2 text-2xl tabular-nums">
          {s.latest.toFixed(s.kind === 'fx' && s.latest < 5 ? 3 : 1)}
          <Badge variant={Math.abs(s.change_6m_pct) >= 10 ? 'destructive' : 'secondary'}>{signed(s.change_6m_pct)}% in 6 months</Badge>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <ChartContainer config={config} className="h-28 w-full">
          <LineChart data={s.points.map(([month, value]) => ({ month, value: Number(value.toFixed(3)) }))}>
            <CartesianGrid vertical={false} />
            <XAxis dataKey="month" hide />
            <YAxis domain={['auto', 'auto']} hide />
            <ChartTooltip content={<ChartTooltipContent />} />
            <Line dataKey="value" stroke="var(--color-value)" strokeWidth={2} dot={false} />
          </LineChart>
        </ChartContainer>
        <div className="text-muted-foreground mt-1 text-xs">{s.unit}; {signed(s.change_since_start_pct)}% since {s.points[0]?.[0]}</div>
      </CardContent>
    </Card>
  )
}

const currencyColumns: ColumnDef<CurrencyExposure['by_currency'][number]>[] = [
  { accessorKey: 'code', header: 'Currency' },
  { accessorKey: 'spend_eur', header: 'Annual spend', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { accessorKey: 'share_pct', header: 'Share', cell: (c) => pct(c.getValue<number>(), 1) },
  { accessorKey: 'contracts', header: 'Contracts' },
  { id: 'exposed', header: 'Exchange-rate risk', accessorFn: (r) => (r.foreign ? 1 : 0), cell: (c) => (c.getValue<number>() ? <Badge variant="destructive">exposed</Badge> : <Badge variant="secondary">reporting currency</Badge>) },
]

export default function Markets() {
  const series = useQuery({ queryKey: ['series'], queryFn: api.series })
  const currency = useQuery({ queryKey: ['currency-exposure'], queryFn: api.currencyExposure })
  const [bump, setBump] = useState(10)
  const sens = useQuery({ queryKey: ['sensitivity', bump], queryFn: () => api.sensitivity(bump), enabled: bump > 0 })

  const sensColumns = (s: Sensitivity): ColumnDef<Sensitivity['rows'][number]>[] => [
    { accessorKey: 'label', header: 'Profit center' },
    { accessorKey: 'linked_material_share_pct', header: 'Material cost on an index', cell: (c) => pct(c.getValue<number>(), 0) },
    ...s.indices.map<ColumnDef<Sensitivity['rows'][number]>>((idx) => ({
      id: idx,
      header: idx,
      accessorFn: (r) => r.per_index[idx] ?? 0,
      cell: (c) => eurCompact.format(c.getValue<number>()),
    })),
    { accessorKey: 'all_indices_extra_cost', header: 'All together', cell: (c) => eurCompact.format(c.getValue<number>()) },
    { accessorKey: 'all_indices_pct_of_revenue', header: '% of revenue', cell: (c) => pct(c.getValue<number>(), 2) },
  ]

  return (
    <div className="space-y-4">
      <PageHeader title="Commodities and FX" subtitle="Market indices behind the price clauses, and how exposed each part of the business is." />
      <Tabs defaultValue="prices">
        <TabsList>
          <TabsTrigger value="prices">Prices</TabsTrigger>
          <TabsTrigger value="commodity">Commodity sensitivity</TabsTrigger>
          <TabsTrigger value="fx">Currency exposure</TabsTrigger>
        </TabsList>
        <TabsContent value="prices" className="space-y-3">
          <Loading query={series} />
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {(series.data ?? []).map((s) => <SeriesCard key={s.iri} s={s} />)}
          </div>
        </TabsContent>
        <TabsContent value="commodity" className="space-y-3">
          <Field label="Index move (%)">
            <input className={`${fieldClass} w-24`} type="number" min={1} value={bump} onChange={(e) => setBump(Number(e.target.value))} />
          </Field>
          <p className="text-muted-foreground text-sm">Extra annual material cost per profit center if an index rises by {bump}%, one index at a time and all together. Sorted by the all-together effect as a share of revenue.</p>
          <Loading query={sens} />
          {sens.data && <DataTable columns={sensColumns(sens.data)} data={sens.data.rows} />}
        </TabsContent>
        <TabsContent value="fx" className="space-y-3">
          <Loading query={currency} />
          {currency.data && (
            <>
              <div className="grid gap-4 sm:grid-cols-3">
                <Kpi title="Annual material spend" value={eurCompact.format(currency.data.total_spend_eur)} />
                <Kpi title="Invoiced in foreign currency" value={pct(currency.data.foreign_share_pct, 1)} hint="Swedish krona and Czech koruna" tone="bad" />
                <Kpi title="As of" value={currency.data.asof} />
              </div>
              <DataTable columns={currencyColumns} data={currency.data.by_currency} />
            </>
          )}
        </TabsContent>
      </Tabs>
    </div>
  )
}
