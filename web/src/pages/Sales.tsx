import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useState } from 'react'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  api,
  eur,
  eurCompact,
  fmt,
  pct,
  signed,
  type CampaignCapacity,
  type FinancingComparison,
  type LeadPressure,
} from '@/lib/api'

const money = (c: { getValue: () => unknown }) => eurCompact.format(c.getValue() as number)
const exact = (c: { getValue: () => unknown }) => eur.format(c.getValue() as number)
const units = (c: { getValue: () => unknown }) => fmt.format(Math.round(c.getValue() as number))

const leadColumns: ColumnDef<LeadPressure['leads'][number]>[] = [
  { id: 'customer', header: 'Customer', accessorFn: (r) => r.customer.replace(/^Synthetic customer /, '') },
  { accessorKey: 'model', header: 'Model' },
  { accessorKey: 'margin_pct', header: 'Margin', cell: (c) => pct(c.getValue<number>()) },
  { accessorKey: 'margin_change_pp', header: 'Margin change (pp)', cell: (c) => signed(c.getValue<number>()) },
  { id: 'pressure', header: 'Pressure', accessorFn: (r) => (r.pressure ? 1 : 0), cell: (c) => (c.row.original.pressure ? <Badge variant="destructive">pressured</Badge> : <Badge variant="outline">ok</Badge>) },
  {
    id: 'reasons',
    header: 'Reasons',
    enableSorting: false,
    accessorFn: (r) => r.reasons.length,
    cell: (c) => (
      <div className="flex flex-wrap gap-1">
        {c.row.original.reasons.length ? c.row.original.reasons.map((x) => <Badge key={x} variant="outline">{x}</Badge>) : <span className="text-muted-foreground">none</span>}
      </div>
    ),
  },
  { accessorKey: 'revenue', header: 'Revenue', cell: money },
]

const campaignColumns: ColumnDef<CampaignCapacity['campaigns'][number]>[] = [
  { accessorKey: 'label', header: 'Campaign' },
  { accessorKey: 'model', header: 'Model' },
  { accessorKey: 'spend', header: 'Spend', cell: money },
  { accessorKey: 'planned_units', header: 'Planned units', cell: units },
  { accessorKey: 'capacity_units', header: 'Capacity units', cell: units },
  { accessorKey: 'utilisation_pct', header: 'Utilisation', cell: (c) => pct(c.getValue<number>()) },
  { accessorKey: 'headroom_units', header: 'Headroom units', cell: units },
  { id: 'cap', header: 'At capacity', accessorFn: (r) => (r.at_capacity ? 1 : 0), cell: (c) => (c.row.original.at_capacity ? <Badge variant="destructive">at capacity</Badge> : <Badge variant="outline">ok</Badge>) },
  { accessorKey: 'wasted_spend', header: 'Wasted spend', cell: money },
]

const productColumns: ColumnDef<FinancingComparison['products'][number]>[] = [
  { accessorKey: 'label', header: 'Product' },
  { accessorKey: 'kind', header: 'Kind', cell: (c) => <Badge variant="outline">{c.getValue<string>()}</Badge> },
  { accessorKey: 'term_months', header: 'Term (months)' },
  { accessorKey: 'rate_pct', header: 'Rate', cell: (c) => pct(c.getValue<number>(), 2) },
  { accessorKey: 'income', header: 'Income', cell: exact },
  { accessorKey: 'funding_cost', header: 'Funding cost', cell: exact },
  { accessorKey: 'risk_cost', header: 'Risk cost', cell: exact },
  { accessorKey: 'subvention_cost', header: 'Subvention cost', cell: exact },
  { accessorKey: 'profit', header: 'Profit per contract', cell: exact },
  { accessorKey: 'profit_pct_of_price', header: 'Profit % of price', cell: (c) => pct(c.getValue<number>(), 2) },
]

const fallbackModels = ['Taycan', 'ID4', 'Golf', 'e-tron', 'Enyaq', 'Octavia']
const fallbackMarkets = ['Czech Republic', 'Germany', 'France']

function Number_({ label, value, set, step = 1, min = 0 }: { label: string; value: number; set: (n: number) => void; step?: number; min?: number }) {
  return (
    <Field label={label}>
      <input className={`${fieldClass} w-24`} type="number" step={step} min={min} value={value} onChange={(e) => set(Number(e.target.value))} />
    </Field>
  )
}

export default function Sales() {
  const refs = useQuery({ queryKey: ['strategy-refs'], queryFn: api.strategyRefs })
  const [floor, setFloor] = useState(15)
  const [buffer, setBuffer] = useState(1)
  const [months, setMonths] = useState(6)
  const leads = useQuery({ queryKey: ['sales-leads', floor, buffer, months], queryFn: () => api.leads(floor, buffer, months), enabled: floor >= 0 && floor < 100 && buffer >= 0 && months >= 1 })
  const [threshold, setThreshold] = useState(95)
  const camps = useQuery({ queryKey: ['sales-campaigns', threshold], queryFn: () => api.campaigns(threshold), enabled: threshold > 0 && threshold <= 200 })
  const [model, setModel] = useState('Taycan')
  const [market, setMarket] = useState('Czech Republic')
  const fin = useQuery({ queryKey: ['sales-financing', model, market], queryFn: () => api.financing(model, market), enabled: model !== '' && market !== '' })
  const markets = refs.data?.markets.map((m) => m.country) ?? fallbackMarkets

  return (
    <div className="space-y-4">
      <PageHeader title="Sales and leads" subtitle="High-intent leads, campaign spend against plant capacity, and lease versus loan. Inputs and assumptions are illustrative; all data is synthetic." />
      <Tabs defaultValue="leads">
        <TabsList>
          <TabsTrigger value="leads">High-intent leads</TabsTrigger>
          <TabsTrigger value="campaigns">Campaigns vs capacity</TabsTrigger>
          <TabsTrigger value="financing">Lease vs loan</TabsTrigger>
        </TabsList>

        <TabsContent value="leads" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Number_ label="Margin floor (%)" value={floor} set={setFloor} />
            <Number_ label="Buffer (points)" value={buffer} set={setBuffer} step={0.5} />
            <Number_ label="Look-back (months)" value={months} set={setMonths} min={1} />
          </div>
          <p className="text-muted-foreground text-sm">A lead is on a pressured model when the margin is under the floor plus buffer, or has fallen over the look-back period.</p>
          <Loading query={leads} />
          {leads.data && (
            <>
              <div className="grid gap-3 sm:grid-cols-2">
                <Kpi title="High-intent leads" value={fmt.format(leads.data.leads.length)} />
                <Kpi title="Leads on pressured models" value={fmt.format(leads.data.pressured_count)} tone="bad" />
              </div>
              <DataTable columns={leadColumns} data={leads.data.leads} />
            </>
          )}
        </TabsContent>

        <TabsContent value="campaigns" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Number_ label="Capacity threshold (%)" value={threshold} set={setThreshold} />
          </div>
          <p className="text-muted-foreground text-sm">Campaign spend is wasted when the model is already at or above the capacity threshold, so extra demand cannot be built.</p>
          <Loading query={camps} />
          {camps.data && (
            <>
              <div className="grid gap-3 sm:grid-cols-2">
                <Kpi title="Total campaign spend" value={eurCompact.format(camps.data.total_spend)} />
                <Kpi title="Spend on capacity-constrained models" value={eurCompact.format(camps.data.wasted_spend)} tone="bad" />
              </div>
              <DataTable columns={campaignColumns} data={camps.data.campaigns} />
            </>
          )}
        </TabsContent>

        <TabsContent value="financing" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Field label="Model">
              <select className={fieldClass} value={model} onChange={(e) => setModel(e.target.value)}>
                {fallbackModels.map((m) => <option key={m} value={m}>{m}</option>)}
              </select>
            </Field>
            <Field label="Market">
              <select className={fieldClass} value={market} onChange={(e) => setMarket(e.target.value)}>
                {markets.map((m) => <option key={m} value={m}>{m}</option>)}
              </select>
            </Field>
          </div>
          <p className="text-muted-foreground text-sm">Assumes vehicle margin is the same under both; amortising loans use an average balance factor.</p>
          <Loading query={fin} />
          {fin.data && (
            <>
              <div className="grid gap-3 sm:grid-cols-2">
                <Kpi title="Best product" value={fin.data.best} hint={`${fin.data.best_kind} on a ${eur.format(fin.data.price)} ${fin.data.model}`} />
                <Kpi title={`Profit difference vs best ${fin.data.best_kind === 'Lease' ? 'loan' : 'lease'}`} value={eur.format(fin.data.kind_gap)} />
              </div>
              <DataTable columns={productColumns} data={fin.data.products} />
            </>
          )}
        </TabsContent>
      </Tabs>
    </div>
  )
}
