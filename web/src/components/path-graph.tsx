// Layered diagram of the knowledge-graph paths behind a margin trace:
// Model -> Component -> Supply contract -> Price index.
import { signed, type MarginTrace } from '@/lib/api'
import { cn } from '@/lib/utils'

type Comp = MarginTrace['components'][number]

const COLS = ['Model', 'Component', 'Supply contract', 'Price index'] as const
const NODE_W = 176
const NODE_H = 34
const GAP_X = 56
const GAP_Y = 12
const PAD = 12
const HEAD = 24

function tidy(col: number, raw: string): string {
  if (col === 1) return raw.replace(/\s*\(illustrative\)$/, '')
  if (col === 2) return raw.replace(/^SupplyContract_\d+_/, '').replace(/_/g, ' ').replace(/([a-z])([A-Z])/g, '$1 $2')
  if (col === 3) return raw.replace(/^Index_/, '').replace(/([a-z])([A-Z])/g, '$1 $2').replace(/_/g, ' ')
  return raw
}

function clip(s: string, n = 25) {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s
}

interface Node {
  key: string
  col: number
  label: string
  x: number
  y: number
  delta?: number
}

export function PathGraph({ components }: { components: Comp[] }) {
  // collect unique nodes per column (in first-seen order) and unique edges
  const cols: string[][] = [[], [], [], []]
  const edges = new Set<string>()
  const delta = new Map<string, number>()
  for (const c of components) {
    for (const p of c.path) {
      const parts = p.split(' -> ')
      parts.forEach((name, i) => {
        if (i < 4 && !cols[i].includes(name)) cols[i].push(name)
        if (i === 1) delta.set(name, c.delta_eur_per_unit)
        if (i > 0 && i < 4) edges.add(`${i - 1}|${parts[i - 1]}|${i}|${name}`)
      })
    }
  }
  if (cols[0].length === 0) return <p className="text-muted-foreground text-sm">No paths for this model.</p>

  const rows = Math.max(...cols.map((c) => c.length))
  const bodyH = rows * NODE_H + (rows - 1) * GAP_Y
  const width = PAD * 2 + COLS.length * NODE_W + (COLS.length - 1) * GAP_X
  const height = PAD * 2 + HEAD + bodyH

  const pos = new Map<string, Node>()
  cols.forEach((names, col) => {
    const colH = names.length * NODE_H + (names.length - 1) * GAP_Y
    const top = PAD + HEAD + (bodyH - colH) / 2
    names.forEach((name, i) => {
      pos.set(`${col}|${name}`, {
        key: `${col}|${name}`,
        col,
        label: tidy(col, name),
        x: PAD + col * (NODE_W + GAP_X),
        y: top + i * (NODE_H + GAP_Y),
        delta: col === 1 ? delta.get(name) : undefined,
      })
    })
  })

  return (
    <div className="overflow-x-auto rounded-lg border">
      <svg role="img" aria-label="Knowledge graph paths from model to price index" viewBox={`0 0 ${width} ${height}`} className="h-auto w-full min-w-[720px]">
        {COLS.map((c, i) => (
          <text key={c} x={PAD + i * (NODE_W + GAP_X) + NODE_W / 2} y={PAD + 8} textAnchor="middle" className="fill-muted-foreground text-[11px] font-medium tracking-wide uppercase">
            {c}
          </text>
        ))}
        {[...edges].map((e) => {
          const [c1, n1, c2, n2] = e.split('|')
          const a = pos.get(`${c1}|${n1}`)
          const b = pos.get(`${c2}|${n2}`)
          if (!a || !b) return null
          const x1 = a.x + NODE_W
          const y1 = a.y + NODE_H / 2
          const x2 = b.x
          const y2 = b.y + NODE_H / 2
          const mx = (x1 + x2) / 2
          return <path key={e} d={`M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`} fill="none" className="stroke-muted-foreground/50" strokeWidth={1.5} />
        })}
        {[...pos.values()].map((n) => {
          const up = n.delta !== undefined && n.delta > 0.005
          const down = n.delta !== undefined && n.delta < -0.005
          return (
            <g key={n.key} transform={`translate(${n.x},${n.y})`}>
              <title>{n.label}{n.delta !== undefined ? ` (${signed(n.delta)} EUR per car)` : ''}</title>
              <rect
                width={NODE_W}
                height={NODE_H}
                rx={8}
                className={cn(
                  'fill-card stroke-border',
                  n.col === 0 && 'fill-primary stroke-primary',
                  n.col === 3 && 'fill-muted',
                  up && 'stroke-destructive',
                  down && 'stroke-emerald-600',
                )}
                strokeWidth={up || down ? 2 : 1}
              />
              <text x={NODE_W / 2} y={n.delta !== undefined ? 14 : NODE_H / 2 + 4} textAnchor="middle" className={cn('text-xs font-medium', n.col === 0 ? 'fill-primary-foreground' : 'fill-foreground')}>
                {clip(n.label)}
              </text>
              {n.delta !== undefined && (
                <text x={NODE_W / 2} y={27} textAnchor="middle" className={cn('text-[10px] tabular-nums', up ? 'fill-destructive' : down ? 'fill-emerald-600' : 'fill-muted-foreground')}>
                  {signed(n.delta)} EUR per car
                </text>
              )}
            </g>
          )
        })}
      </svg>
    </div>
  )
}
