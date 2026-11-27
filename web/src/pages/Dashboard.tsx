import { useQuery } from '@tanstack/react-query'
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from 'recharts'
import { Kpi, Loading, PageHeader } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { api, eurCompact, fmt, pct, signed } from '@/lib/api'

const config = { margin: { label: 'Contribution margin %', color: 'var(--chart-1)' } } satisfies ChartConfig

export default function Dashboard() {
  const dash = useQuery({ queryKey: ['dashboard'], queryFn: api.dashboard })
  const d = dash.data
  return (
    <div className="space-y-6">
      <PageHeader
        title="Dashboard"
        subtitle={d ? `Position as of ${d.asof}. Synthetic, illustrative data.` : 'Synthetic, illustrative data.'}
      />
      <Loading query={dash} />
      {d && (
        <>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Kpi title="Planned revenue" value={eurCompact.format(d.planned_revenue)} hint="price x planned annual production" />
            <Kpi title="Contribution margin" value={pct(d.contribution_margin_pct)} hint="after material, labour, logistics, warranty, marketing" />
            <Kpi title="Material spend" value={eurCompact.format(d.material_spend)} hint={`${pct(d.commodity_linked_share_pct, 0)} follows a commodity index`} />
            <Kpi title="Spend in foreign currency" value={pct(d.foreign_currency_share_pct, 0)} hint="SEK and CZK contracts" />
          </div>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Kpi title="Contracts" value={fmt.format(d.contracts)} hint={`${d.contracts_under_committed} projected above commitment`} />
            <Kpi title="Expiring within 12 months" value={fmt.format(d.contracts_expiring_12m)} hint="see Contracts" tone={d.contracts_expiring_12m > 0 ? 'bad' : undefined} />
            <Kpi title="Single-sourced components" value={fmt.format(d.single_sourced_components)} hint="no qualified alternative supplier" tone="bad" />
            <Kpi title="Largest 6-month mover" value={d.movers[0].label} hint={`${signed(d.movers[0].change_6m_pct)}%`} />
          </div>
          <div className="grid gap-4 lg:grid-cols-3">
            <Card className="lg:col-span-2">
              <CardHeader>
                <CardTitle>Contribution margin by profit center</CardTitle>
                <CardDescription>At current commodity prices and exchange rates</CardDescription>
              </CardHeader>
              <CardContent>
                <ChartContainer config={config} className="h-64 w-full">
                  <BarChart data={d.margin_by_profit_center.map(([label, margin]) => ({ label: label.replace(' profit center', ''), margin }))}>
                    <CartesianGrid vertical={false} />
                    <XAxis dataKey="label" tickLine={false} axisLine={false} />
                    <YAxis tickLine={false} axisLine={false} width={36} unit="%" />
                    <ChartTooltip content={<ChartTooltipContent />} />
                    <Bar dataKey="margin" fill="var(--color-margin)" radius={4} />
                  </BarChart>
                </ChartContainer>
              </CardContent>
            </Card>
            <Card>
              <CardHeader>
                <CardTitle>Market movers</CardTitle>
                <CardDescription>Change over the last 6 months</CardDescription>
              </CardHeader>
              <CardContent className="space-y-2">
                {d.movers.map((m) => (
                  <div key={m.label} className="flex items-center justify-between text-sm">
                    <span>{m.label}</span>
                    <Badge variant={Math.abs(m.change_6m_pct) >= 10 ? 'destructive' : 'secondary'}>{signed(m.change_6m_pct)}%</Badge>
                  </div>
                ))}
              </CardContent>
            </Card>
          </div>
        </>
      )}
    </div>
  )
}
