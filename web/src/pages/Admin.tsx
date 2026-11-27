import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { DataTable } from '@/components/data-table'
import { Badge } from '@/components/ui/badge'
import { api, fmt, type GraphStat } from '@/lib/api'

const columns: ColumnDef<GraphStat>[] = [
  { accessorKey: 'file', header: 'File' },
  { accessorKey: 'triples', header: 'Triples', cell: (c) => fmt.format(c.getValue<number>()) },
  { accessorKey: 'graph', header: 'Named graph' },
]

export default function Admin() {
  const health = useQuery({ queryKey: ['health'], queryFn: api.health, refetchInterval: 10_000 })
  const graphs = useQuery({ queryKey: ['graphs'], queryFn: api.graphs })
  return (
    <div className="space-y-4">
      <h1 className="text-2xl font-semibold">Admin</h1>
      <div className="flex items-center gap-2 text-sm">
        SQLite
        <Badge variant={health.data?.sqlite === 'ok' ? 'secondary' : 'destructive'}>{health.data?.sqlite ?? '…'}</Badge>
        Knowledge graph
        <Badge variant="secondary">{fmt.format(health.data?.kg_triples ?? 0)} triples</Badge>
      </div>
      <DataTable columns={columns} data={graphs.data ?? []} />
    </div>
  )
}
