import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { useState } from 'react'
import { DataTable } from '@/components/data-table'
import { Field, Kpi, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { api, eur, eurCompact, fmt, num, shortName, type ExpiringContract, type Row, type Savings } from '@/lib/api'

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

export default function Contracts() {
  const [months, setMonths] = useState(12)
  const contracts = useQuery({ queryKey: ['contracts'], queryFn: api.contracts })
  const expiring = useQuery({ queryKey: ['expiring', months], queryFn: () => api.expiring(months) })
  const savings = useQuery({ queryKey: ['savings'], queryFn: api.savings })
  const needAction = expiring.data?.contracts.filter((c) => c.under_committed).length ?? 0

  return (
    <div className="space-y-4">
      <PageHeader title="Contracts" subtitle="Supply contracts: review flags from the rules, upcoming expiries and volume-discount savings." />
      <Tabs defaultValue="review">
        <TabsList>
          <TabsTrigger value="review">Review</TabsTrigger>
          <TabsTrigger value="expiring">Expiring</TabsTrigger>
          <TabsTrigger value="savings">Volume discounts</TabsTrigger>
        </TabsList>
        <TabsContent value="review" className="space-y-3">
          <Loading query={contracts} />
          <DataTable columns={reviewColumns} data={contracts.data ?? []} />
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
          <DataTable columns={expiringColumns} data={expiring.data?.contracts ?? []} />
        </TabsContent>
        <TabsContent value="savings" className="space-y-3">
          {savings.data && <Kpi title="Saving if every reached tier is applied" value={eurCompact.format(savings.data.total_annual_saving)} hint="(current price minus tier price) x projected volume; assumes the tier price applies to all units" />}
          <Loading query={savings} />
          <DataTable columns={savingColumns} data={savings.data?.rows ?? []} />
        </TabsContent>
      </Tabs>
    </div>
  )
}
