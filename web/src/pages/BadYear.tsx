import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  api,
  eurCompact,
  fmt,
  pct,
  shortName,
  type CombinedRequest,
  type CombinedResult,
  type SavedCombined,
  type ShockSpec,
} from '@/lib/api'

type Brand = CombinedResult['by_brand'][number]
type TopModel = CombinedResult['top_models'][number]
type ContractRow = CombinedResult['contracts'][number]

const money = (v: number) => eurCompact.format(v)

const brandColumns: ColumnDef<Brand>[] = [
  { accessorKey: 'label', header: 'Brand' },
  { accessorKey: 'contribution_before', header: 'Contribution before', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'extra_material_cost', header: 'Extra material cost', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'lost_contribution', header: 'Lost contribution', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'profit_change', header: 'Profit change', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'profit_change_pct', header: 'Profit change % of contribution', cell: (c) => pct(c.getValue<number>(), 2) },
]

const modelColumns: ColumnDef<TopModel>[] = [
  { accessorKey: 'label', header: 'Model', cell: (c) => <Link className="underline-offset-2 hover:underline" to={`/explain/${shortName(c.row.original.iri)}`}>{c.getValue<string>()}</Link> },
  { accessorKey: 'brand', header: 'Brand' },
  { accessorKey: 'extra_material_cost', header: 'Extra material cost', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'lost_units', header: 'Cars lost', cell: (c) => fmt.format(Math.round(c.getValue<number>())) },
  { accessorKey: 'lost_contribution', header: 'Lost contribution', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'profit_change', header: 'Profit change', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'margin_before_pct', header: 'Margin before', cell: (c) => pct(c.getValue<number>(), 2) },
  { accessorKey: 'margin_after_pct', header: 'Margin after', cell: (c) => pct(c.getValue<number>(), 2) },
  {
    accessorKey: 'below_floor',
    header: 'Floor',
    cell: (c) => (c.getValue<boolean>() ? <Badge variant="destructive">below floor</Badge> : <Badge variant="secondary">ok</Badge>),
  },
]

const contractColumns: ColumnDef<ContractRow>[] = [
  { accessorKey: 'priority', header: 'Priority' },
  { accessorKey: 'label', header: 'Contract' },
  { accessorKey: 'supplier', header: 'Supplier' },
  { accessorKey: 'extra_cost', header: 'Extra cost / year', cell: (c) => money(c.getValue<number>()) },
  { accessorKey: 'sourcing', header: 'Sourcing' },
  { accessorKey: 'end', header: 'Ends' },
  { accessorKey: 'months_to_end', header: 'Months to end' },
  {
    id: 'drivers',
    header: 'Drivers',
    enableSorting: false,
    cell: (c) => (
      <div className="flex flex-wrap gap-1">
        {c.row.original.drivers.map((d) => (
          <Badge key={d.shock} variant="outline">{`${d.shock}: ${money(d.extra_cost)}`}</Badge>
        ))}
      </div>
    ),
  },
]

function describe(r: CombinedResult): string {
  const parts = r.shocks.map((s) => `${shortName(s.target).replace(/^Index_/, '')} ${s.percent > 0 ? '+' : ''}${s.percent}%`)
  if (r.subsidy_markets.length) parts.push(`EV subsidy ends in ${r.subsidy_markets.join(', ')}`)
  return `Scenario: ${parts.length ? parts.join(', ') : 'no shocks'} (elasticity ${r.elasticity}, margin floor ${pct(r.margin_floor_pct, 0)}).`
}

const hasTarget = (s: ShockSpec, name: string) => shortName(s.target).replace(/^Index_/, '') === name

export default function BadYear() {
  const qc = useQueryClient()
  const [rubber, setRubber] = useState(15)
  const [krona, setKrona] = useState(5)
  const [subsidy, setSubsidy] = useState(true)
  const [elasticity, setElasticity] = useState(1.5)
  const [floor, setFloor] = useState(15)
  const [saveAs, setSaveAs] = useState('')
  const [result, setResult] = useState<CombinedResult | null>(null)

  const shocks: ShockSpec[] = []
  if (rubber !== 0) shocks.push({ kind: 'commodity', target: 'NaturalRubber', percent: rubber })
  if (krona !== 0) shocks.push({ kind: 'fx', target: 'SEK', percent: krona })
  const nothing = shocks.length === 0 && !subsidy

  const run = useMutation({
    mutationFn: () =>
      api.runCombined({
        shocks,
        subsidy_markets: subsidy ? ['Germany'] : [],
        elasticity,
        margin_floor: floor / 100,
        save_as: saveAs || undefined,
      }),
    onSuccess: (data) => {
      setResult(data.result)
      if (data.id) {
        setSaveAs('')
        void qc.invalidateQueries({ queryKey: ['saved-combined'] })
      }
    },
  })

  const started = useRef(false)
  useEffect(() => {
    if (started.current) return
    started.current = true
    run.mutate()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const saved = useQuery({ queryKey: ['saved-combined'], queryFn: api.savedCombined })

  const load = async (id: number) => {
    const r = await api.savedCombinedRun(id)
    setResult(r.result)
    const req: CombinedRequest = r.request
    setRubber(req.shocks.find((s) => s.kind === 'commodity' && hasTarget(s, 'NaturalRubber'))?.percent ?? 0)
    setKrona(req.shocks.find((s) => s.kind === 'fx' && hasTarget(s, 'SEK'))?.percent ?? 0)
    setSubsidy((req.subsidy_markets ?? []).length > 0)
    if (req.elasticity !== undefined) setElasticity(req.elasticity)
    if (req.margin_floor !== undefined) setFloor(Math.round(req.margin_floor * 100))
  }

  const savedColumns: ColumnDef<SavedCombined>[] = [
    { accessorKey: 'name', header: 'Name' },
    { accessorKey: 'created_at', header: 'Created', cell: (c) => c.getValue<string>().slice(0, 16).replace('T', ' ') },
    { id: 'profit_change', header: 'Profit change', accessorFn: (r) => r.totals.profit_change, cell: (c) => money(c.getValue<number>()) },
    { id: 'extra', header: 'Extra material cost', accessorFn: (r) => r.totals.extra_material_cost, cell: (c) => money(c.getValue<number>()) },
    { id: 'lost', header: 'Lost contribution', accessorFn: (r) => r.totals.lost_contribution, cell: (c) => money(c.getValue<number>()) },
    {
      id: 'load',
      header: '',
      enableSorting: false,
      cell: (c) => <Button variant="outline" size="sm" onClick={() => void load(c.row.original.id)}>Load</Button>,
    },
  ]

  return (
    <div className="space-y-6">
      <PageHeader
        title="Bad year"
        subtitle="Combines commodity, currency and subsidy shocks in one run. Elasticity and shock sizes are inputs; data is synthetic."
      />
      <Card>
        <CardContent className="flex flex-wrap items-end gap-3">
          <Field label="Natural rubber change %">
            <input className={`${fieldClass} w-28`} type="number" step="1" value={rubber} onChange={(e) => setRubber(Number(e.target.value))} />
          </Field>
          <Field label="Swedish krona change vs EUR %">
            <input className={`${fieldClass} w-28`} type="number" step="1" value={krona} onChange={(e) => setKrona(Number(e.target.value))} />
          </Field>
          <Field label="Price elasticity">
            <input className={`${fieldClass} w-24`} type="number" step="0.1" value={elasticity} onChange={(e) => setElasticity(Number(e.target.value))} />
          </Field>
          <Field label="Margin floor %">
            <input className={`${fieldClass} w-24`} type="number" value={floor} onChange={(e) => setFloor(Number(e.target.value))} />
          </Field>
          <Field label="Save as (optional)">
            <input className={fieldClass} value={saveAs} onChange={(e) => setSaveAs(e.target.value)} placeholder="name to keep this run" />
          </Field>
          <label className="flex h-8 items-center gap-2 text-sm">
            <input type="checkbox" checked={subsidy} onChange={(e) => setSubsidy(e.target.checked)} />
            Germany ends EV subsidy
          </label>
          <Button onClick={() => run.mutate()} disabled={run.isPending || nothing}>{run.isPending ? 'Running…' : 'Run'}</Button>
        </CardContent>
      </Card>
      {run.error && <p className="text-destructive text-sm">{run.error.message}</p>}
      {result ? <ResultsWithSaved r={result} saved={saved} columns={savedColumns} /> : run.isPending ? <p className="text-muted-foreground text-sm">Running…</p> : null}
    </div>
  )
}

/** Results plus the Saved runs tab, which lives in the same Tabs group. */
function ResultsWithSaved({ r, saved, columns }: { r: CombinedResult; saved: { data?: SavedCombined[]; isLoading: boolean; error: Error | null }; columns: ColumnDef<SavedCombined>[] }) {
  const t = r.totals
  return (
    <div className="space-y-4">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Kpi title="Profit change" value={money(t.profit_change)} hint={`${pct(t.profit_change_pct, 2)} of contribution`} tone={t.profit_change < 0 ? 'bad' : undefined} />
        <Kpi title="Extra material cost" value={money(t.extra_material_cost)} hint={`prices as of ${r.asof}`} />
        <Kpi title="Contribution lost to demand" value={money(t.lost_contribution)} hint={`${money(t.lost_revenue)} revenue`} />
        <Kpi title="Cars lost" value={fmt.format(Math.round(t.lost_units))} />
      </div>
      <p className="text-sm">{describe(r)}</p>
      <Tabs defaultValue="brand">
        <TabsList>
          <TabsTrigger value="brand">By brand</TabsTrigger>
          <TabsTrigger value="models">Top 5 models</TabsTrigger>
          <TabsTrigger value="contracts">Contracts to renegotiate first</TabsTrigger>
          <TabsTrigger value="saved">Saved runs</TabsTrigger>
        </TabsList>
        <TabsContent value="brand"><DataTable columns={brandColumns} data={r.by_brand} /></TabsContent>
        <TabsContent value="models"><DataTable columns={modelColumns} data={r.top_models} /></TabsContent>
        <TabsContent value="contracts"><DataTable columns={contractColumns} data={r.contracts} /></TabsContent>
        <TabsContent value="saved">
          <Loading query={saved} />
          {saved.data && (saved.data.length ? <DataTable columns={columns} data={saved.data} /> : <p className="text-muted-foreground text-sm">No saved runs yet. Use "Save as" when running.</p>)}
        </TabsContent>
      </Tabs>
    </div>
  )
}
