import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useState } from 'react'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { api, eurCompact, fmt, pct, shortName, type LeadTimeImpact, type SingleSource, type SupplierRisk } from '@/lib/api'

const list = (v: string[]) => (
  <div className="flex flex-wrap gap-1">{v.length ? v.map((x) => <Badge key={x} variant="outline">{x}</Badge>) : <span className="text-muted-foreground">none</span>}</div>
)

const riskColumns: ColumnDef<SupplierRisk>[] = [
  { accessorKey: 'label', header: 'Supplier' },
  { accessorKey: 'country', header: 'Country' },
  { accessorKey: 'revenue_at_risk', header: 'Revenue depending on it', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { accessorKey: 'revenue_share_pct', header: 'Share of revenue', cell: (c) => pct(c.getValue<number>(), 0) },
  { id: 'models', header: 'Models', accessorFn: (r) => r.models.length },
  { accessorKey: 'single_sourced_components', header: 'Single-sourced parts' },
  { accessorKey: 'lead_time_days', header: 'Lead time (days)' },
  { id: 'alt', header: 'Alternative suppliers', enableSorting: false, accessorFn: (r) => r.alternatives.length, cell: (c) => list(c.row.original.alternatives) },
]

const singleColumns: ColumnDef<SingleSource>[] = [
  { accessorKey: 'label', header: 'Component' },
  { accessorKey: 'supplier', header: 'Only supplier' },
  { accessorKey: 'revenue_dependent', header: 'Revenue depending on it', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { id: 'models', header: 'Models', enableSorting: false, accessorFn: (r) => r.models.length, cell: (c) => list(c.row.original.models) },
  { accessorKey: 'lead_time_days', header: 'Lead time (days)' },
  { accessorKey: 'contract_end', header: 'Contract ends' },
]

const leadColumns: ColumnDef<LeadTimeImpact['models'][number]>[] = [
  { accessorKey: 'label', header: 'Model' },
  { id: 'via', header: 'Through', enableSorting: false, accessorFn: (r) => r.via_components.length, cell: (c) => list(c.row.original.via_components) },
  { accessorKey: 'planned_launch', header: 'Planned launch', cell: (c) => c.getValue<string | null>() ?? 'no launch planned' },
  { accessorKey: 'new_launch', header: 'New date', cell: (c) => c.getValue<string | null>() ?? '' },
  { accessorKey: 'revenue_at_risk', header: 'Revenue delayed', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { id: 'leads', header: 'High-intent leads', enableSorting: false, accessorFn: (r) => r.high_intent_leads.length, cell: (c) => list(c.row.original.high_intent_leads.map((l) => l.replace(/^Synthetic customer /, ''))) },
]

export default function Suppliers() {
  const risk = useQuery({ queryKey: ['supplier-risk'], queryFn: api.supplierRisk })
  const single = useQuery({ queryKey: ['single-sourced'], queryFn: api.singleSourced })
  const [supplier, setSupplier] = useState('Supplier_BatterySystems')
  const [factor, setFactor] = useState(2)
  const lead = useQuery({ queryKey: ['lead-time', supplier, factor], queryFn: () => api.leadTime(supplier, factor), enabled: factor >= 1 })

  return (
    <div className="space-y-4">
      <PageHeader title="Suppliers" subtitle="Who the business depends on, which parts have no second source, and what a longer lead time would delay." />
      <Tabs defaultValue="risk">
        <TabsList>
          <TabsTrigger value="risk">Dependency</TabsTrigger>
          <TabsTrigger value="single">Single-sourced parts</TabsTrigger>
          <TabsTrigger value="lead">Lead-time shock</TabsTrigger>
        </TabsList>
        <TabsContent value="risk" className="space-y-3">
          <p className="text-muted-foreground text-sm">Revenue is price x planned production of every model that uses a part from the supplier (each model counted once). It is the revenue exposed if that supplier stopped delivering.</p>
          <Loading query={risk} />
          <DataTable columns={riskColumns} data={risk.data ?? []} />
        </TabsContent>
        <TabsContent value="single" className="space-y-3">
          {single.data && <Kpi title="Components with a single source and no alternative" value={fmt.format(single.data.length)} tone="bad" />}
          <Loading query={single} />
          <DataTable columns={singleColumns} data={single.data ?? []} />
        </TabsContent>
        <TabsContent value="lead" className="space-y-3">
          <div className="flex flex-wrap items-end gap-3">
            <Field label="Supplier">
              <select className={fieldClass} value={supplier} onChange={(e) => setSupplier(e.target.value)}>
                {(risk.data ?? []).map((s) => (
                  <option key={s.iri} value={shortName(s.iri)}>{s.label}</option>
                ))}
              </select>
            </Field>
            <Field label="Lead time multiplied by">
              <input className={`${fieldClass} w-24`} type="number" step="0.5" min={1} value={factor} onChange={(e) => setFactor(Number(e.target.value))} />
            </Field>
          </div>
          <Loading query={lead} />
          {lead.data && (
            <>
              <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                <Kpi title="Lead time" value={`${lead.data.lead_time_days} to ${lead.data.new_lead_time_days} days`} hint={`${lead.data.delay_days} days of delay`} />
                <Kpi title="Models affected" value={String(lead.data.models.length)} />
                <Kpi title="Revenue delayed (upper bound)" value={eurCompact.format(lead.data.total_revenue_at_risk)} hint="annual revenue of the models x delay / 365" tone="bad" />
                <Kpi title="High-intent leads affected" value={String(lead.data.leads_affected)} />
              </div>
              <DataTable columns={leadColumns} data={lead.data.models} />
            </>
          )}
        </TabsContent>
      </Tabs>
    </div>
  )
}
