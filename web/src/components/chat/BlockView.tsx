// Renders the structured results that chat tools return.
import { Link } from 'react-router'
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from 'recharts'
import { Button } from '@/components/ui/button'
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { ChatBlock } from '@/lib/api'
import { cn } from '@/lib/utils'

const config = { value: { label: 'Value', color: 'var(--chart-1)' } } satisfies ChartConfig

export function BlockView({ block, onNavigate }: { block: ChatBlock; onNavigate?: () => void }) {
  switch (block.type) {
    case 'kpis':
      return (
        <div className="grid grid-cols-2 gap-2">
          {block.items.map((k) => (
            <div key={k.label} className="bg-background rounded-lg border p-2">
              <div className="text-muted-foreground text-xs">{k.label}</div>
              <div className={cn('text-lg font-semibold tabular-nums', k.tone === 'bad' && 'text-destructive')}>{k.value}</div>
              {k.hint && <div className="text-muted-foreground text-xs">{k.hint}</div>}
            </div>
          ))}
        </div>
      )
    case 'table':
      return (
        <div className="bg-background rounded-lg border">
          <div className="px-2 pt-2 text-xs font-medium">{block.title}</div>
          <div className="overflow-x-auto">
            <Table className="text-xs">
              <TableHeader>
                <TableRow>
                  {block.columns.map((c) => (
                    <TableHead key={c} className="h-8 px-2 whitespace-nowrap">{c}</TableHead>
                  ))}
                </TableRow>
              </TableHeader>
              <TableBody>
                {block.rows.map((row, i) => (
                  <TableRow key={i}>
                    {row.map((cell, j) => (
                      <TableCell key={j} className="px-2 py-1.5 whitespace-nowrap">{cell}</TableCell>
                    ))}
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </div>
      )
    case 'bars':
      return (
        <div className="bg-background rounded-lg border p-2">
          <div className="text-xs font-medium">{block.title} <span className="text-muted-foreground font-normal">({block.unit})</span></div>
          <ChartContainer config={config} className="mt-1 w-full" style={{ height: 26 * block.items.length + 34 }}>
            <BarChart data={block.items} layout="vertical" margin={{ left: 0, right: 12 }}>
              <CartesianGrid horizontal={false} />
              <XAxis type="number" tickLine={false} axisLine={false} tick={{ fontSize: 10 }} />
              <YAxis type="category" dataKey="label" tickLine={false} axisLine={false} width={96} interval={0} tick={{ fontSize: 10 }} />
              <ChartTooltip content={<ChartTooltipContent />} />
              <Bar dataKey="value" fill="var(--color-value)" radius={3} />
            </BarChart>
          </ChartContainer>
        </div>
      )
    case 'link':
      return (
        <Button variant="outline" size="sm" render={<Link to={block.to} state={block.shocks ? { shocks: block.shocks } : undefined} onClick={onNavigate} />}>
          {block.label}
        </Button>
      )
  }
}
