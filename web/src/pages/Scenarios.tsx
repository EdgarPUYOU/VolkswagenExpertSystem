import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useState } from 'react'
import { Link, useLocation } from 'react-router'
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from 'recharts'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  api,
  eur,
  eurCompact,
  pct,
  shortName,
  signed,
  type ContractImpact,
  type ModelImpact,
  type ProfitCenterImpact,
  type ScenarioResult,
  type ShockSpec,
} from '@/lib/api'

const PRESETS: { name: string; shocks: ShockSpec[] }[] = [
  { name: 'Rubber +15%', shocks: [{ kind: 'commodity', target: 'Index_NaturalRubber', percent: 15 }] },
  {
    name: 'Steel +10%, aluminium +8%',
    shocks: [
      { kind: 'commodity', target: 'Index_Steel', percent: 10 },
      { kind: 'commodity', target: 'Index_Aluminium', percent: 8 },
    ],
  },
  { name: 'Krona +5%', shocks: [{ kind: 'fx', target: 'SEK', percent: 5 }] },
  {
    name: 'Rough quarter',
    shocks: [
      { kind: 'commodity', target: 'Index_NaturalRubber', percent: 15 },
      { kind: 'commodity', target: 'Index_Steel', percent: 10 },
      { kind: 'fx', target: 'SEK', percent: 5 },
    ],
  },
]

/** Targets may arrive as 'NaturalRubber' (chat) or 'Index_NaturalRubber' (saved runs); the selects use the latter. */
function normaliseShock(s: ShockSpec): ShockSpec {
  const target = shortName(s.target)
  return s.kind === 'commodity' && !target.startsWith('Index_') ? { ...s, target: `Index_${target}` } : { ...s, target }
}

const chartConfig = { extra: { label: 'Extra annual cost', color: 'var(--chart-1)' } } satisfies ChartConfig

const pcColumns: ColumnDef<ProfitCenterImpact>[] = [
  { accessorKey: 'label', header: 'Profit center' },
  { accessorKey: 'extra_cost', header: 'Extra cost / year', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { accessorKey: 'extra_cost_pct_of_budget', header: '% of material budget', cell: (c) => pct(c.getValue<number>(), 2) },
  { accessorKey: 'extra_cost_pct_of_revenue', header: '% of revenue', cell: (c) => pct(c.getValue<number>(), 3) },
  { accessorKey: 'margin_before_pct', header: 'Margin before', cell: (c) => pct(c.getValue<number>(), 2) },
  { accessorKey: 'margin_after_pct', header: 'Margin after', cell: (c) => pct(c.getValue<number>(), 2) },
]

const modelColumns: ColumnDef<ModelImpact>[] = [
  { accessorKey: 'label', header: 'Model', cell: (c) => <Link className="underline-offset-2 hover:underline" to={`/explain/${shortName(c.row.original.iri)}`}>{c.getValue<string>()}</Link> },
  { accessorKey: 'brand', header: 'Brand' },
  { accessorKey: 'extra_cost', header: 'Extra cost / year', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { accessorKey: 'margin_before_pct', header: 'Margin before', cell: (c) => pct(c.getValue<number>(), 2) },
  { accessorKey: 'margin_after_pct', header: 'Margin after', cell: (c) => pct(c.getValue<number>(), 2) },
  {
    id: 'flag',
    header: 'Floor',
    accessorFn: (r) => (r.crossed_floor ? 2 : r.below_floor ? 1 : 0),
    cell: (c) => {
      const v = c.getValue<number>()
      return v === 2 ? <Badge variant="destructive">falls below</Badge> : v === 1 ? <Badge variant="outline">already below</Badge> : <Badge variant="secondary">ok</Badge>
    },
  },
]

const contractColumns: ColumnDef<ContractImpact>[] = [
  { accessorKey: 'label', header: 'Contract' },
  { accessorKey: 'supplier', header: 'Supplier' },
  { accessorKey: 'price_before', header: 'Unit price', cell: (c) => eur.format(c.getValue<number>()) },
  { accessorKey: 'price_after', header: 'After', cell: (c) => eur.format(c.getValue<number>()) },
  { accessorKey: 'extra_cost', header: 'Extra cost / year', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { accessorKey: 'sourcing', header: 'Sourcing' },
  { accessorKey: 'end', header: 'Ends' },
]

function Results({ r }: { r: ScenarioResult }) {
  const t = r.totals
  return (
    <div className="space-y-4">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Kpi title="Extra material cost / year" value={eurCompact.format(t.extra_cost)} hint={`on ${eurCompact.format(t.revenue)} planned revenue`} tone="bad" />
        <Kpi title="Group margin" value={`${pct(t.margin_before_pct, 2)} to ${pct(t.margin_after_pct, 2)}`} hint={`${signed(t.margin_after_pct - t.margin_before_pct, 2)} points`} />
        <Kpi title={`Models below ${pct(r.margin_floor_pct, 0)} margin`} value={String(t.models_below_floor)} hint={`${t.models_crossing_floor} fall below because of this scenario`} tone={t.models_crossing_floor > 0 ? 'bad' : undefined} />
        <Kpi title="Contracts affected" value={String(r.by_contract.length)} hint={`prices as of ${r.asof}`} />
      </div>
      <Card>
        <CardHeader>
          <CardTitle>Extra cost by profit center</CardTitle>
        </CardHeader>
        <CardContent>
          <ChartContainer config={chartConfig} className="h-52 w-full">
            <BarChart data={r.by_profit_center.map((p) => ({ label: p.label.replace(' profit center', ''), extra: Math.round(p.extra_cost) }))}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="label" tickLine={false} axisLine={false} />
              <YAxis tickLine={false} axisLine={false} width={56} tickFormatter={(v: number) => eurCompact.format(v)} />
              <ChartTooltip content={<ChartTooltipContent />} />
              <Bar dataKey="extra" fill="var(--color-extra)" radius={4} />
            </BarChart>
          </ChartContainer>
        </CardContent>
      </Card>
      <Tabs defaultValue="pc">
        <TabsList>
          <TabsTrigger value="pc">Profit centers</TabsTrigger>
          <TabsTrigger value="models">Models</TabsTrigger>
          <TabsTrigger value="contracts">Contracts</TabsTrigger>
        </TabsList>
        <TabsContent value="pc"><DataTable columns={pcColumns} data={r.by_profit_center} /></TabsContent>
        <TabsContent value="models"><DataTable columns={modelColumns} data={r.by_model} /></TabsContent>
        <TabsContent value="contracts">
          <DataTable columns={contractColumns} data={r.by_contract} />
          <p className="text-muted-foreground mt-2 text-xs">
            Each contract's cost is split by shock when several are combined: {r.by_contract[0]?.drivers.map((d) => `${d.shock} ${eurCompact.format(d.extra_cost)}`).join(', ')} (largest contract shown).
          </p>
        </TabsContent>
      </Tabs>
    </div>
  )
}

export default function Scenarios() {
  const qc = useQueryClient()
  const series = useQuery({ queryKey: ['series'], queryFn: api.series })
  const saved = useQuery({ queryKey: ['saved-scenarios'], queryFn: api.savedScenarios })
  const location = useLocation()
  // the chat bubble can open this page with the shocks it just ran
  const [shocks, setShocks] = useState<ShockSpec[]>(() => {
    const fromChat = (location.state as { shocks?: ShockSpec[] } | null)?.shocks
    return fromChat?.length ? fromChat.map(normaliseShock) : PRESETS[0].shocks
  })
  const [floor, setFloor] = useState(15)
  const [saveAs, setSaveAs] = useState('')
  const [result, setResult] = useState<ScenarioResult | null>(null)

  const run = useMutation({
    mutationFn: () => api.runScenario({ shocks, margin_floor: floor / 100, save_as: saveAs || undefined }),
    onSuccess: (data) => {
      setResult(data.result)
      if (data.id) {
        setSaveAs('')
        void qc.invalidateQueries({ queryKey: ['saved-scenarios'] })
      }
    },
  })

  const indices = (series.data ?? []).filter((s) => s.kind === 'index')
  const currencies = (series.data ?? []).filter((s) => s.kind === 'fx')
  const optionsFor = (kind: ShockSpec['kind']) =>
    kind === 'commodity'
      ? indices.map((s) => ({ value: shortName(s.iri), label: s.label }))
      : currencies.map((s) => ({ value: shortName(s.iri), label: s.label.replace('EUR/', '') }))
  const update = (i: number, patch: Partial<ShockSpec>) => setShocks((cur) => cur.map((s, j) => (j === i ? { ...s, ...patch } : s)))

  return (
    <div className="space-y-6">
      <PageHeader
        title="Scenario lab"
        subtitle="Move commodity prices or exchange rates and see the effect on cost, margin and contracts. Percent changes apply to current month prices."
      />
      <div className="grid gap-4 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle>Shocks</CardTitle>
            <CardDescription>
              {PRESETS.map((p) => (
                <Button key={p.name} variant="outline" size="sm" className="mr-2 mt-1" onClick={() => setShocks(p.shocks)}>
                  {p.name}
                </Button>
              ))}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {shocks.map((s, i) => (
              <div key={i} className="flex flex-wrap items-end gap-3">
                <Field label="Type">
                  <select
                    className={fieldClass}
                    value={s.kind}
                    onChange={(e) => {
                      const kind = e.target.value as ShockSpec['kind']
                      update(i, { kind, target: optionsFor(kind)[0]?.value ?? '' })
                    }}
                  >
                    <option value="commodity">Commodity index</option>
                    <option value="fx">Currency vs EUR</option>
                  </select>
                </Field>
                <Field label={s.kind === 'fx' ? 'Currency (+ = stronger)' : 'Index'}>
                  <select className={fieldClass} value={s.target} onChange={(e) => update(i, { target: e.target.value })}>
                    {optionsFor(s.kind).map((o) => (
                      <option key={o.value} value={o.value}>{o.label}</option>
                    ))}
                  </select>
                </Field>
                <Field label="Change %">
                  <input className={`${fieldClass} w-24`} type="number" step="1" value={s.percent} onChange={(e) => update(i, { percent: Number(e.target.value) })} />
                </Field>
                <Button variant="ghost" size="sm" disabled={shocks.length === 1} onClick={() => setShocks((cur) => cur.filter((_, j) => j !== i))}>
                  Remove
                </Button>
              </div>
            ))}
            <Button variant="outline" size="sm" onClick={() => setShocks((cur) => [...cur, { kind: 'commodity', target: indices[0] ? shortName(indices[0].iri) : '', percent: 10 }])}>
              Add shock
            </Button>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Run</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <Field label="Margin floor %">
              <input className={`${fieldClass} w-24`} type="number" value={floor} onChange={(e) => setFloor(Number(e.target.value))} />
            </Field>
            <Field label="Save as (optional)">
              <input className={fieldClass} value={saveAs} onChange={(e) => setSaveAs(e.target.value)} placeholder="name to keep this run" />
            </Field>
            <Button onClick={() => run.mutate()} disabled={run.isPending}>{run.isPending ? 'Running…' : 'Run scenario'}</Button>
            {run.error && <p className="text-destructive text-sm">{run.error.message}</p>}
            {saved.data && saved.data.length > 0 && (
              <div className="space-y-1 pt-2">
                <div className="text-muted-foreground text-xs">Saved runs</div>
                {saved.data.slice(0, 6).map((s) => (
                  <Button
                    key={s.id}
                    variant="ghost"
                    size="sm"
                    className="w-full justify-start"
                    onClick={async () => {
                      const r = await api.savedScenario(s.id)
                      setResult(r.result)
                      setShocks(r.result.shocks.map(normaliseShock))
                    }}
                  >
                    {s.name}
                  </Button>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
      {result && <Results r={result} />}
    </div>
  )
}
