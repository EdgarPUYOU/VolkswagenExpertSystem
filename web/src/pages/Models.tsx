import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { DataTable } from '@/components/data-table'
import { api, eur, fmt, num, type Row } from '@/lib/api'

const columns: ColumnDef<Row>[] = [
  { accessorKey: 'brand', header: 'Brand' },
  { accessorKey: 'label', header: 'Model' },
  { accessorFn: (r) => num(r.price), id: 'price', header: 'Price', cell: (c) => eur.format(c.getValue<number>()) },
  {
    accessorFn: (r) => num(r.planned),
    id: 'planned',
    header: 'Planned / year',
    cell: (c) => fmt.format(c.getValue<number>()),
  },
]

export default function Models() {
  const { data, isLoading } = useQuery({ queryKey: ['models'], queryFn: api.models })
  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-semibold">Portfolio and models</h1>
      {isLoading ? <p>Loading…</p> : <DataTable columns={columns} data={data ?? []} />}
    </div>
  )
}
