import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useEffect, useState } from 'react'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { api, eur, eurCompact, fmt, num, shortName, type ContractDetail, type ExpiringContract, type Row, type Savings } from '@/lib/api'

const REASON_LABEL: Record<string, string> = {
  VolumeExceedsCommitment: 'over commitment',
  VolumeDiscountAvailable: 'discount tier reached',
}

const reviewColumns: ColumnDef<Row>[] = [
  { accessorFn: (r) => shortName(r.contract), id: 'contract', header: 'Contract' },
  { accessorKey: 'supplier', header: 'Supplier' },
  { accessorFn: (r) => num(r.committed), id: 'committed', header: 'Committed / year', cell: (c) => fmt.format(c.getValue<number>()) },
  { accessorFn: (r) => num(r.projected), id: 'projected', header: 'Projected / year', cell: (c) => fmt.format(c.getValue<number>()) },
  { accessorFn: (r) => num(r.price), id: 'price', header: 'Unit price', cell: (c) => eur.format(c.getValue<number>()) },
  {
    id: 'review',
    header: 'Review (from rules)',
    accessorFn: (r) => r.reasons ?? '',
    cell: (c) => {
      const reasons = c.getValue<string>().split(',').filter(Boolean)
      if (reasons.length === 0) return <Badge variant="secondary">ok</Badge>
      return (
        <div className="flex flex-wrap gap-1">
          {reasons.map((r) => (
            <Badge key={r} variant={r.endsWith('VolumeExceedsCommitment') ? 'destructive' : 'outline'}>
              {REASON_LABEL[shortName(r)] ?? shortName(r)}
            </Badge>
          ))}
        </div>
      )
    },
  },
]

const expiringColumns: ColumnDef<ExpiringContract>[] = [
  { accessorKey: 'component', header: 'Component' },
  { accessorKey: 'supplier', header: 'Supplier' },
  { accessorKey: 'end', header: 'Ends' },
  { accessorKey: 'days_left', header: 'Days left' },
  { accessorKey: 'annual_spend', header: 'Annual spend', cell: (c) => eurCompact.format(c.getValue<number>()) },
  { accessorKey: 'sourcing', header: 'Sourcing' },
  {
    id: 'volume',
    header: 'Volume',
    accessorFn: (r) => (r.under_committed ? 1 : 0),
    cell: (c) => {
      const r = c.row.original
      return r.under_committed ? (
        <Badge variant="destructive">projected {fmt.format(r.projected)} vs committed {fmt.format(r.committed)}</Badge>
      ) : (
        <Badge variant="secondary">within commitment</Badge>
      )
    },
  },
]

const savingColumns: ColumnDef<Savings['rows'][number]>[] = [
  { accessorKey: 'component', header: 'Component' },
  { accessorKey: 'supplier', header: 'Supplier' },
  { accessorKey: 'projected', header: 'Projected volume', cell: (c) => fmt.format(c.getValue<number>()) },
  { accessorKey: 'tier', header: 'Tier at', cell: (c) => fmt.format(c.getValue<number>()) },
  { accessorKey: 'current_price', header: 'Price', cell: (c) => eur.format(c.getValue<number>()) },
  { accessorKey: 'discount_price', header: 'Tier price', cell: (c) => eur.format(c.getValue<number>()) },
  { accessorKey: 'annual_saving', header: 'Saving / year', cell: (c) => eurCompact.format(c.getValue<number>()) },
]

function Item({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="text-sm font-medium">{children}</dd>
    </div>
  )
}

function ContractDialog({ iri, onClose }: { iri: string; onClose: () => void }) {
  const detail = useQuery({ queryKey: ['contract', iri], queryFn: () => api.contractDetail(iri) })
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])
  const d: ContractDetail | undefined = detail.data
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4" onClick={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        className="bg-background max-h-[90vh] w-full max-w-3xl space-y-4 overflow-y-auto rounded-xl border p-5 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-lg font-semibold">{d?.label ?? shortName(iri)}</h2>
            <p className="text-muted-foreground text-xs">{shortName(iri)}</p>
          </div>
          <button className="text-muted-foreground hover:text-foreground text-xl leading-none" onClick={onClose} aria-label="Close">×</button>
        </div>
        <Loading query={detail} />
        {d && (
          <>
            <div className="flex flex-wrap gap-1">
              {d.review_reasons.length === 0 ? (
                <Badge variant="secondary">no review flags</Badge>
              ) : (
                d.review_reasons.map((r) => (
                  <Badge key={r} variant={r.endsWith('VolumeExceedsCommitment') ? 'destructive' : 'outline'}>
                    {REASON_LABEL[shortName(r)] ?? shortName(r)}
                  </Badge>
                ))
              )}
            </div>
            <dl className="grid grid-cols-2 gap-3 sm:grid-cols-3">
              <Item label="Supplier">{d.supplier}</Item>
              <Item label="Component">{d.component}</Item>
              <Item label="Sourcing">{d.sourcing}</Item>
              <Item label="Start">{d.start}</Item>
              <Item label="End">{d.end}</Item>
              <Item label="Time left">{d.days_left < 0 ? `expired ${-d.days_left} days ago` : `${fmt.format(d.days_left)} days`}</Item>
              <Item label="Currency">{shortName(d.currency)}</Item>
              <Item label="Annual spend">{eurCompact.format(d.annual_spend)}</Item>
              <Item label="Alternative suppliers">{d.alternatives.length ? d.alternatives.join(', ') : 'none'}</Item>
            </dl>
            <h3 className="text-sm font-semibold">Volume and price</h3>
            <dl className="grid grid-cols-2 gap-3 sm:grid-cols-3">
              <Item label="Committed / year">{fmt.format(d.committed)}</Item>
              <Item label="Projected / year">{fmt.format(d.projected)}</Item>
              <Item label="Projected vs committed">{d.utilisation_pct.toFixed(0)}%</Item>
              <Item label="Base price">{eur.format(d.base_price)}</Item>
              <Item label="Price today (after indexation)">{eur.format(d.current_price)}</Item>
              <Item label="Volume discount">
                {d.discount_price != null && d.tier != null ? `${eur.format(d.discount_price)} from ${fmt.format(d.tier)} units` : 'none'}
              </Item>
            </dl>
            <h3 className="text-sm font-semibold">Price indexation clauses</h3>
            {d.clauses.length === 0 ? (
              <p className="text-muted-foreground text-sm">Fixed price, no index clauses.</p>
            ) : (
              <DataTable
                columns={[
                  { accessorKey: 'index', header: 'Index' },
                  { accessorKey: 'weight', header: 'Weight', cell: (c) => `${(c.getValue<number>() * 100).toFixed(0)}%` },
                  { accessorKey: 'pass_through', header: 'Pass-through', cell: (c) => `${(c.getValue<number>() * 100).toFixed(0)}%` },
                ] as ColumnDef<ContractDetail['clauses'][number]>[]}
                data={d.clauses}
              />
            )}
            <h3 className="text-sm font-semibold">Models covered</h3>
            <DataTable
              columns={[
                { accessorKey: 'label', header: 'Model' },
                { accessorKey: 'brand', header: 'Brand' },
                { accessorKey: 'planned', header: 'Planned units', cell: (c) => fmt.format(c.getValue<number>()) },
              ] as ColumnDef<ContractDetail['models'][number]>[]}
              data={d.models}
            />
          </>
        )}
      </div>
    </div>
  )
}

export default function Contracts() {
  const [selected, setSelected] = useState<string | null>(null)
  const [months, setMonths] = useState(12)
  const contracts = useQuery({ queryKey: ['contracts'], queryFn: api.contracts })
  const expiring = useQuery({ queryKey: ['expiring', months], queryFn: () => api.expiring(months) })
  const savings = useQuery({ queryKey: ['savings'], queryFn: api.savings })
  const needAction = expiring.data?.contracts.filter((c) => c.under_committed).length ?? 0

  return (
    <div className="space-y-4">
      <PageHeader title="Contracts" subtitle="Click a row for full contract details. Supply contracts: review flags from the rules, upcoming expiries and volume-discount savings." />
      <Tabs defaultValue="review">
        <TabsList>
          <TabsTrigger value="review">Review</TabsTrigger>
          <TabsTrigger value="expiring">Expiring</TabsTrigger>
          <TabsTrigger value="savings">Volume discounts</TabsTrigger>
        </TabsList>
        <TabsContent value="review" className="space-y-3">
          <Loading query={contracts} />
          <DataTable columns={reviewColumns} data={contracts.data ?? []} onRowClick={(r) => setSelected(r.contract)} />
        </TabsContent>
        <TabsContent value="expiring" className="space-y-3">
          <div className="flex flex-wrap items-end gap-4">
            <Field label="Expiring within (months)">
              <input className={`${fieldClass} w-24`} type="number" min={1} max={60} value={months} onChange={(e) => setMonths(Math.max(1, Number(e.target.value)))} />
            </Field>
            {expiring.data && (
              <div className="grid w-full gap-4 sm:w-auto sm:grid-cols-2">
                <Kpi title="Contracts expiring" value={String(expiring.data.contracts.length)} hint={`until ${expiring.data.cutoff}`} />
                <Kpi title="Also under-committed" value={String(needAction)} hint="renegotiate volume first" tone={needAction > 0 ? 'bad' : undefined} />
              </div>
            )}
          </div>
          <Loading query={expiring} />
          <DataTable columns={expiringColumns} data={expiring.data?.contracts ?? []} onRowClick={(r) => setSelected(r.iri)} />
        </TabsContent>
        <TabsContent value="savings" className="space-y-3">
          {savings.data && <Kpi title="Saving if every reached tier is applied" value={eurCompact.format(savings.data.total_annual_saving)} hint="(current price minus tier price) x projected volume; assumes the tier price applies to all units" />}
          <Loading query={savings} />
          <DataTable columns={savingColumns} data={savings.data?.rows ?? []} onRowClick={(r) => setSelected(r.iri)} />
        </TabsContent>
      </Tabs>
      {selected && <ContractDialog iri={selected} onClose={() => setSelected(null)} />}
    </div>
  )
}
