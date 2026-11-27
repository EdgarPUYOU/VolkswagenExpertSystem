import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useState } from 'react'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  api,
  eurCompact,
  fmt,
  pct,
  type CompetitorCut,
  type FleetRow,
  type GroupSites,
  type MixAnalysis,
  type SubsidyRemoval,
} from '@/lib/api'

const money = (c: { getValue: () => unknown }) => eurCompact.format(c.getValue() as number)
const units = (c: { getValue: () => unknown }) => fmt.format(Math.round(c.getValue() as number))
const list = (v: string[]) => (
  <div className="flex flex-wrap gap-1">{v.length ? v.map((x) => <Badge key={x} variant="outline">{x}</Badge>) : <span className="text-muted-foreground">none</span>}</div>
)

const subsidyColumns: ColumnDef<SubsidyRemoval['models'][number]>[] = [
  { accessorKey: 'label', header: 'Model' },
  { accessorKey: 'brand', header: 'Brand' },
  { accessorKey: 'units_in_market', header: 'Planned units here', cell: units },
  { accessorKey: 'subsidy_eur', header: 'Subsidy', cell: money },
  { accessorKey: 'demand_loss_pct', header: 'Demand lost', cell: (c) => pct(c.getValue<number>()) },
  { accessorKey: 'lost_units', header: 'Units lost', cell: units },
  { accessorKey: 'lost_revenue', header: 'Revenue lost', cell: money },
  { accessorKey: 'lost_contribution', header: 'Contribution lost', cell: money },
]

const fleetColumns: ColumnDef<FleetRow>[] = [
  { accessorKey: 'label', header: 'Brand' },
  { accessorKey: 'cars_in_target_markets', header: 'Cars in target markets', cell: units },
  { accessorKey: 'avg_g_per_km', header: 'Fleet g/km', cell: (c) => c.getValue<number>().toFixed(1) },
  { accessorKey: 'penalty_now', header: 'Penalty today', cell: money },
  { accessorKey: 'penalty_new', header: 'Penalty at new target', cell: money },
  { id: 'status', header: 'At new target', accessorFn: (r) => (r.breaches_new ? 1 : 0), cell: (c) => (c.row.original.breaches_new ? <Badge variant="destructive">breach</Badge> : <Badge variant="outline">ok</Badge>) },
]

const hitColumns: ColumnDef<CompetitorCut['hits'][number]>[] = [
  { accessorKey: 'model', header: 'Our model' },
  { accessorKey: 'rival', header: 'Rival' },
  { accessorKey: 'our_price', header: 'Our price', cell: money },
  { accessorKey: 'rival_price_after', header: 'Rival price after cut', cell: money },
  { id: 'undercut', header: 'Undercut', accessorFn: (r) => (r.undercut ? 1 : 0), cell: (c) => (c.row.original.undercut ? <Badge variant="destructive">yes</Badge> : <Badge variant="outline">no</Badge>) },
  { accessorKey: 'volume_loss_pct', header: 'Volume lost', cell: (c) => pct(c.getValue<number>()) },
  { accessorKey: 'revenue_at_risk', header: 'Revenue at risk', cell: money },
  { accessorKey: 'contribution_at_risk', header: 'Contribution at risk', cell: money },
]

const siteColumns: ColumnDef<GroupSites['sites'][number]>[] = [
  { accessorKey: 'site', header: 'Site' },
  { accessorKey: 'brand', header: 'Brand' },
  { accessorKey: 'revenue', header: 'Revenue', cell: money },
  { accessorKey: 'share_pct', header: 'Share of group', cell: (c) => pct(c.getValue<number>(), 0) },
]

const mixColumns: ColumnDef<MixAnalysis['powertrains'][number]>[] = [
  { accessorKey: 'powertrain', header: 'Powertrain' },
  { accessorKey: 'models', header: 'Models' },
  { accessorKey: 'units', header: 'Planned units', cell: units },
  { accessorKey: 'revenue', header: 'Revenue', cell: money },
  { accessorKey: 'margin_pct', header: 'Contribution margin', cell: (c) => pct(c.getValue<number>()) },
  { accessorKey: 'contribution_per_unit', header: 'Per unit', cell: money },
  { id: 'sub', header: 'Subsidy dependent', enableSorting: false, accessorFn: (r) => r.subsidy_dependent.length, cell: (c) => list(c.row.original.subsidy_dependent) },
  { id: 'co2', header: 'Above CO2 target', enableSorting: false, accessorFn: (r) => r.co2_penalty_risk.length, cell: (c) => list(c.row.original.co2_penalty_risk) },
]

const shiftColumns: ColumnDef<MixAnalysis['shifts'][number]>[] = [
  { accessorKey: 'brand', header: 'Brand' },
  { accessorKey: 'moved_units', header: 'Cars moved to electric', cell: units },
  { accessorKey: 'contribution_change', header: 'Contribution change', cell: money },
  { id: 'g', header: 'Fleet g/km', accessorFn: (r) => r.avg_g_after, cell: (c) => `${c.row.original.avg_g_before.toFixed(0)} to ${c.row.original.avg_g_after.toFixed(0)}` },
  { accessorKey: 'co2_penalty_before', header: 'CO2 penalty before', cell: money },
  { accessorKey: 'co2_penalty_after', header: 'CO2 penalty after', cell: money },
  { accessorKey: 'net_change', header: 'Net effect', cell: money },
]

function Number_({ label, value, set, step = 1, min = 0 }: { label: string; value: number; set: (n: number) => void; step?: number; min?: number }) {
  return (
    <Field label={label}>
      <input className={`${fieldClass} w-24`} type="number" step={step} min={min} value={value} onChange={(e) => set(Number(e.target.value))} />
    </Field>
  )
}

export default function Strategy() {
  const refs = useQuery({ queryKey: ['strategy-refs'], queryFn: api.strategyRefs })
  const [market, setMarket] = useState('Germany')
  const [elasticity, setElasticity] = useState(1.5)
  const subsidy = useQuery({ queryKey: ['subsidy', market, elasticity], queryFn: () => api.subsidy(market, elasticity), enabled: elasticity >= 0 })
  const [tighten, setTighten] = useState(10)
  const co2 = useQuery({ queryKey: ['co2', tighten], queryFn: () => api.co2(tighten), enabled: tighten >= 0 && tighten < 100 })
  const [rival, setRival] = useState('')
  const [cut, setCut] = useState(10)
  const [cross, setCross] = useState(0.8)
  const comp = useQuery({ queryKey: ['competitor', rival, cut, cross], queryFn: () => api.competitor(rival, cut, cross), enabled: cut >= 0 && cut < 100 && cross >= 0 })
  const sites = useQuery({ queryKey: ['sites'], queryFn: api.sites })
  const [shift, setShift] = useState(10)
  const mix = useQuery({ queryKey: ['mix', shift], queryFn: () => api.mix(shift), enabled: shift >= 0 && shift <= 100 })

  return (
    <div className="space-y-4">
      <PageHeader title="Markets and regulation" subtitle="Subsidies, CO2 targets, competitor moves, site dependency and powertrain mix. Rates and elasticities are inputs, not facts; all data is synthetic." />
      <Tabs defaultValue="subsidy">
        <TabsList>
          <TabsTrigger value="subsidy">Subsidy ends</TabsTrigger>
          <TabsTrigger value="co2">CO2 fleet target</TabsTrigger>
          <TabsTrigger value="competitor">Competitor price cut</TabsTrigger>
          <TabsTrigger value="sites">Site dependency</TabsTrigger>
          <TabsTrigger value="mix">Powertrain mix</TabsTrigger>
        </TabsList>

        <TabsContent value="subsidy" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Field label="Market">
              <select className={fieldClass} value={market} onChange={(e) => setMarket(e.target.value)}>
                {(refs.data?.markets ?? []).map((m) => <option key={m.iri} value={m.country}>{m.label}</option>)}
              </select>
            </Field>
            <Number_ label="Price elasticity of demand" value={elasticity} set={setElasticity} step={0.1} />
          </div>
          <p className="text-muted-foreground text-sm">Without the subsidy the buyer pays subsidy / price more. Demand falls by elasticity times that share.</p>
          <Loading query={subsidy} />
          {subsidy.data && (
            <>
              <div className="grid gap-3 sm:grid-cols-3">
                <Kpi title="Units lost" value={fmt.format(Math.round(subsidy.data.lost_units))} />
                <Kpi title="Revenue lost" value={eurCompact.format(subsidy.data.lost_revenue)} tone="bad" />
                <Kpi title="Contribution lost" value={eurCompact.format(subsidy.data.lost_contribution)} tone="bad" />
              </div>
              {subsidy.data.models.length === 0 && <p className="text-muted-foreground text-sm">No subsidy applies in {subsidy.data.market}.</p>}
              <DataTable columns={subsidyColumns} data={subsidy.data.models} />
              <div>
                <h3 className="mb-1 text-sm font-medium">High-intent leads affected</h3>
                {list(subsidy.data.leads.map((l) => `${l.customer.replace(/^Synthetic customer /, '')}: ${l.model}`))}
              </div>
            </>
          )}
        </TabsContent>

        <TabsContent value="co2" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Number_ label="Target lowered by (%)" value={tighten} set={setTighten} />
          </div>
          <Loading query={co2} />
          {co2.data && (
            <>
              <p className="text-muted-foreground text-sm">
                {co2.data.regulation}: {co2.data.target_now} g/km becomes {co2.data.target_new.toFixed(1)} g/km. Penalty is EUR {co2.data.penalty_per_gram} per g/km per car above target. Passenger cars only, in the markets the target covers.
              </p>
              <div className="grid gap-3 sm:grid-cols-3">
                <Kpi title="Brands in breach today" value={String(co2.data.brands.filter((b) => b.penalty_now > 0).length)} />
                <Kpi title="Brands in breach at new target" value={String(co2.data.brands.filter((b) => b.breaches_new).length)} tone="bad" />
                <Kpi title="Penalty at new target (brands alone)" value={eurCompact.format(co2.data.brands.reduce((a, b) => a + b.penalty_new, 0))} hint={`Pooled as one fleet: ${eurCompact.format(co2.data.pooled.penalty_new)}`} tone="bad" />
              </div>
              <DataTable columns={fleetColumns} data={[...co2.data.brands, co2.data.pooled]} />
            </>
          )}
        </TabsContent>

        <TabsContent value="competitor" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Field label="Competitor">
              <select className={fieldClass} value={rival} onChange={(e) => setRival(e.target.value)}>
                <option value="">All electric rivals</option>
                {(refs.data?.competitors ?? []).map((c) => <option key={c} value={c}>{c}</option>)}
              </select>
            </Field>
            <Number_ label="Price cut (%)" value={cut} set={setCut} />
            <Number_ label="Cross elasticity" value={cross} set={setCross} step={0.1} />
          </div>
          <p className="text-muted-foreground text-sm">A model is exposed when a rival of its segment ends up priced below it. Its volume then falls by cross elasticity times the cut.</p>
          <Loading query={comp} />
          {comp.data && (
            <>
              <div className="grid gap-3 sm:grid-cols-3">
                <Kpi title="Revenue exposed" value={eurCompact.format(comp.data.revenue_exposed)} />
                <Kpi title="Revenue at risk" value={eurCompact.format(comp.data.revenue_at_risk)} tone="bad" />
                <Kpi title="Contribution at risk" value={eurCompact.format(comp.data.contribution_at_risk)} tone="bad" />
              </div>
              <DataTable columns={hitColumns} data={comp.data.hits} />
            </>
          )}
        </TabsContent>

        <TabsContent value="sites" className="space-y-4">
          <p className="text-muted-foreground text-sm">Revenue is price x planned production. A group depends on a site when most of its revenue is built there.</p>
          <Loading query={sites} />
          {(sites.data ?? []).map((g) => (
            <div key={g.group} className="space-y-2">
              <h3 className="text-sm font-medium">{g.group}: {pct(g.top_share_pct, 0)} of {eurCompact.format(g.revenue)} at {g.top_site}</h3>
              <DataTable columns={siteColumns} data={g.sites} />
            </div>
          ))}
        </TabsContent>

        <TabsContent value="mix" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Number_ label="Combustion cars moved to electric (%)" value={shift} set={setShift} />
          </div>
          <p className="text-muted-foreground text-sm">Moves the share of a brand's combustion volume to its own electric models, pro rata to their planned volume. Demand is assumed to follow.</p>
          <Loading query={mix} />
          {mix.data && (
            <>
              <DataTable columns={mixColumns} data={mix.data.powertrains} />
              <h3 className="text-sm font-medium">What if {mix.data.shift_pct}% of combustion cars moved to electric</h3>
              <DataTable columns={shiftColumns} data={mix.data.shifts} />
            </>
          )}
        </TabsContent>
      </Tabs>
    </div>
  )
}
