import { useQuery } from '@tanstack/react-query'
import cytoscape from 'cytoscape'
import dagre from 'cytoscape-dagre'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { Field, Loading, PageHeader, fieldClass } from '@/components/ui-bits'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { api, type ExplainGraph, type GraphEdge, type GraphEntity, type GraphNode, type Origin, type OriginKind } from '@/lib/api'

cytoscape.use(dagre)

const DEFAULT_PAIR = { from: 'http://example.org/volkswagen/Index_NaturalRubber', to: 'http://example.org/volkswagen/Golf' }

type Pair = { from: string; to: string }

/** Node box from the label length: one line up to the wrap width, then one more line per wrap. */
function nodeSize(label: string): { w: number; h: number } {
  const text = label.length * 6.4
  const lines = Math.max(1, Math.ceil(text / 140))
  return { w: Math.max(64, Math.min(140, text) + 24), h: 22 + lines * 14 }
}

// ---- colours (oklch hue per group; lightness and chroma depend on the theme)

const GROUP_HUE: Record<string, number> = {
  index: 250,
  currency: 200,
  clause: 300,
  contract: 330,
  component: 70,
  supplier: 40,
  model: 145,
  brand: 170,
  profit_center: 110,
  group: 270,
  site: 20,
  market: 220,
  regulation: 0,
  competitor: 350,
  customer: 90,
  campaign: 130,
  financing: 180,
  powertrain: 60,
  region: 100,
  other: 0,
}
const GROUPS = Object.keys(GROUP_HUE)
const ROUNDED_GROUPS = new Set(['contract', 'clause'])

const groupKey = (g: string) => (g in GROUP_HUE ? g : 'other')
const fill = (g: string, dark: boolean) => {
  const key = groupKey(g)
  const c = key === 'other' ? 0 : dark ? 0.08 : 0.07
  return `oklch(${dark ? 0.38 : 0.88} ${c} ${GROUP_HUE[key]})`
}
const stroke = (g: string, dark: boolean) => {
  const key = groupKey(g)
  const c = key === 'other' ? 0 : 0.13
  return `oklch(${dark ? 0.62 : 0.55} ${c} ${GROUP_HUE[key]})`
}
const INFERRED = { light: 'oklch(0.6 0.19 50)', dark: 'oklch(0.78 0.16 70)' }
const ERP = { light: 'oklch(0.55 0.11 195)', dark: 'oklch(0.75 0.11 195)' }

// Canvas cannot read CSS variables or oklch(), so resolve any CSS colour to rgb().
let probe: CanvasRenderingContext2D | null = null
function toRgb(color: string): string {
  try {
    probe ??= Object.assign(document.createElement('canvas'), { width: 1, height: 1 }).getContext('2d', { willReadFrequently: true })
    if (!probe) return color
    probe.clearRect(0, 0, 1, 1)
    probe.fillStyle = '#000'
    probe.fillStyle = color
    probe.fillRect(0, 0, 1, 1)
    const [r, g, b, a] = probe.getImageData(0, 0, 1, 1).data
    return `rgba(${r},${g},${b},${a / 255})`
  } catch {
    return color
  }
}
function cssVar(name: string): string {
  const el = document.createElement('span')
  el.style.color = `var(${name})`
  document.body.appendChild(el)
  const c = getComputedStyle(el).color
  el.remove()
  return toRgb(c)
}

function useIsDark(): boolean {
  const [dark, setDark] = useState(() => document.documentElement.classList.contains('dark'))
  useEffect(() => {
    const el = document.documentElement
    const obs = new MutationObserver(() => setDark(el.classList.contains('dark')))
    obs.observe(el, { attributes: true, attributeFilter: ['class'] })
    return () => obs.disconnect()
  }, [])
  return dark
}

// ---- helpers

const tail = (iri: string) => iri.split(/[/#]/).filter(Boolean).pop() ?? iri
const short = (s: string) => (s.includes('#') || s.includes('/') ? tail(s) : s)
const dec = (s: string) => {
  try {
    return decodeURIComponent(s)
  } catch {
    return s
  }
}
const enc = encodeURIComponent

function parsePairs(raw: string | null): Pair[] {
  if (!raw) return [DEFAULT_PAIR]
  const out = raw
    .split(',')
    .map((p) => p.split('|'))
    .filter((p) => p.length === 2 && p[0] && p[1])
    .map(([from, to]) => ({ from: dec(from), to: dec(to) }))
  return out.length ? out : [DEFAULT_PAIR]
}
const serialise = (pairs: Pair[]) => pairs.map((p) => `${enc(p.from)}|${enc(p.to)}`).join(',')

function originText(o: Origin): string {
  if (o.kind === 'inferred') return `inferred by ${o.source}`
  if (o.kind === 'erp_fact') return `ERP fact from ${o.source}`
  return `asserted in ${o.source}`
}
const kindLabel: Record<OriginKind, string> = { asserted: 'asserted', inferred: 'inferred', erp_fact: 'ERP fact' }

// ---- entity picker

function EntityPicker({ label, value, onPick }: { label: string; value: { iri: string; label: string }; onPick: (e: GraphEntity) => void }) {
  const [text, setText] = useState('')
  const [open, setOpen] = useState(false)
  const [debounced, setDebounced] = useState('')
  useEffect(() => {
    const t = setTimeout(() => setDebounced(text.trim()), 200)
    return () => clearTimeout(t)
  }, [text])
  const res = useQuery({ queryKey: ['graph-entities', debounced], queryFn: () => api.graphEntities(debounced), enabled: open && debounced.length >= 1 })
  return (
    <div className="relative flex min-w-56 flex-1 flex-col gap-1 text-xs">
      <label className="flex flex-col gap-1">
        <span className="text-muted-foreground">{label}</span>
        <input
          className={fieldClass}
          value={open ? text : value.label}
          placeholder="Type to search entities"
          role="combobox"
          aria-expanded={open}
          aria-autocomplete="list"
          onFocus={() => {
            setText('')
            setOpen(true)
          }}
          onBlur={() => setTimeout(() => setOpen(false), 150)}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      {open && debounced.length >= 1 && (
        <ul role="listbox" className="bg-popover text-popover-foreground absolute top-full z-20 mt-1 max-h-64 w-full overflow-auto rounded-lg border p-1 shadow-md">
          {res.isLoading && <li className="text-muted-foreground px-2 py-1">Searching…</li>}
          {res.error && <li className="text-destructive px-2 py-1">{res.error.message}</li>}
          {res.data?.length === 0 && <li className="text-muted-foreground px-2 py-1">No match</li>}
          {res.data?.map((e) => (
            <li key={e.iri} role="option" aria-selected={e.iri === value.iri}>
              <button
                type="button"
                className="hover:bg-accent flex w-full items-center justify-between gap-2 rounded-md px-2 py-1 text-left"
                onMouseDown={(ev) => ev.preventDefault()}
                onClick={() => {
                  onPick(e)
                  setOpen(false)
                }}
              >
                <span className="truncate">{e.label}</span>
                <Badge variant="outline">{e.group}</Badge>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

// ---- graph canvas

type Selection = { kind: 'node' | 'edge'; id: string } | null
type CanvasApi = { fit: () => void; zoom: (f: number) => void }

function buildStyle(dark: boolean): cytoscape.StylesheetJson {
  const fg = cssVar('--foreground')
  const muted = cssVar('--muted-foreground')
  const bg = cssVar('--background')
  const ring = cssVar('--primary')
  const inferred = toRgb(dark ? INFERRED.dark : INFERRED.light)
  const erp = toRgb(dark ? ERP.dark : ERP.light)
  const style: cytoscape.StylesheetJson = [
    {
      selector: 'node',
      style: {
        label: 'data(label)',
        shape: 'ellipse',
        width: 'data(w)',
        height: 'data(h)',
        padding: '10px',
        'text-valign': 'center',
        'text-halign': 'center',
        'text-wrap': 'wrap',
        'text-max-width': '140px',
        'font-size': 11,
        color: fg,
        'background-color': toRgb(fill('other', dark)),
        'border-width': 1.5,
        'border-color': toRgb(stroke('other', dark)),
      },
    },
    {
      selector: 'edge',
      style: {
        label: 'data(label)',
        width: 1.8,
        'curve-style': 'bezier',
        'target-arrow-shape': 'triangle',
        'arrow-scale': 1.1,
        'line-color': muted,
        'target-arrow-color': muted,
        'font-size': 9,
        color: muted,
        'text-rotation': 'autorotate',
        'text-background-color': bg,
        'text-background-opacity': 0.85,
        'text-background-padding': '2px',
        'text-margin-y': -2,
      },
    },
    { selector: 'edge[kind = "asserted"]', style: { 'line-style': 'solid' } },
    { selector: 'edge[kind = "inferred"]', style: { 'line-style': 'dashed', 'line-color': inferred, 'target-arrow-color': inferred, color: inferred, width: 2.2 } },
    { selector: 'edge[kind = "erp_fact"]', style: { 'line-style': 'dotted', 'line-color': erp, 'target-arrow-color': erp, color: erp, width: 2.2 } },
    { selector: '.dim', style: { opacity: 0.15 } },
    { selector: 'edge.hl', style: { width: 3.5, 'z-index': 10 } },
    { selector: 'node.hl', style: { 'border-width': 3, 'border-color': ring } },
    { selector: ':selected', style: { 'overlay-color': ring, 'overlay-opacity': 0.15, 'overlay-padding': 6 } },
  ]
  for (const g of GROUPS) {
    style.push({
      selector: `node[group = "${g}"]`,
      style: {
        'background-color': toRgb(fill(g, dark)),
        'border-color': toRgb(stroke(g, dark)),
        shape: ROUNDED_GROUPS.has(g) ? 'round-rectangle' : 'ellipse',
      },
    })
  }
  return style
}

function GraphCanvas({
  data,
  highlight,
  onSelect,
  apiRef,
}: {
  data: ExplainGraph
  highlight: { nodes: string[]; edges: string[] } | null
  onSelect: (s: Selection) => void
  apiRef: React.MutableRefObject<CanvasApi | null>
}) {
  const box = useRef<HTMLDivElement>(null)
  const cyRef = useRef<cytoscape.Core | null>(null)
  const dark = useIsDark()
  const onSelectRef = useRef(onSelect)
  onSelectRef.current = onSelect

  useEffect(() => {
    if (!box.current) return
    const cy = cytoscape({ container: box.current, minZoom: 0.1, maxZoom: 3 })
    cyRef.current = cy
    cy.on('tap', (e) => {
      if (e.target === cy) onSelectRef.current(null)
      else if (e.target.isNode()) onSelectRef.current({ kind: 'node', id: e.target.id() })
      else onSelectRef.current({ kind: 'edge', id: e.target.id() })
    })
    apiRef.current = {
      fit: () => cy.fit(undefined, 30),
      zoom: (f) => cy.zoom({ level: cy.zoom() * f, renderedPosition: { x: cy.width() / 2, y: cy.height() / 2 } }),
    }
    const ro = new ResizeObserver(() => {
      cy.resize()
      cy.fit(undefined, 30)
    })
    ro.observe(box.current)
    return () => {
      ro.disconnect()
      apiRef.current = null
      cyRef.current = null
      cy.destroy()
    }
  }, [apiRef])

  useEffect(() => {
    const cy = cyRef.current
    if (cy) cy.style(buildStyle(dark))
  }, [dark])

  useEffect(() => {
    const cy = cyRef.current
    if (!cy) return
    const nodeIds = new Set(data.nodes.map((n) => n.id))
    cy.batch(() => {
      cy.elements().remove()
      cy.add([
        ...data.nodes.map((n) => ({ group: 'nodes' as const, data: { id: n.id, label: n.label, group: n.group, ...nodeSize(n.label) } })),
        ...data.edges
          .filter((e) => nodeIds.has(e.source) && nodeIds.has(e.target))
          .map((e) => ({ group: 'edges' as const, data: { id: e.id, source: e.source, target: e.target, label: e.label, kind: e.origin.kind } })),
      ])
    })
    cy.style(buildStyle(dark))
    const layout = cy.layout({ name: 'dagre', rankDir: 'LR', nodeSep: 30, rankSep: 90, edgeSep: 20, fit: true, padding: 30 } as cytoscape.LayoutOptions)
    layout.one('layoutstop', () => cy.fit(undefined, 30))
    layout.run()
    cy.fit(undefined, 30)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data])

  useEffect(() => {
    const cy = cyRef.current
    if (!cy) return
    cy.batch(() => {
      cy.elements().removeClass('dim hl')
      if (!highlight) return
      const keep = cy.collection()
      for (const id of highlight.nodes) keep.merge(cy.getElementById(id))
      for (const id of highlight.edges) keep.merge(cy.getElementById(id))
      cy.elements().not(keep).addClass('dim')
      keep.addClass('hl')
    })
  }, [highlight, data])

  return <div ref={box} className="bg-card h-[420px] w-full rounded-lg border sm:h-[520px]" />
}

// ---- legend

function Legend({ groups, dark }: { groups: string[]; dark: boolean }) {
  const kinds: { kind: OriginKind; dash: string; color: string }[] = [
    { kind: 'asserted', dash: '', color: 'var(--muted-foreground)' },
    { kind: 'inferred', dash: '6 3', color: dark ? INFERRED.dark : INFERRED.light },
    { kind: 'erp_fact', dash: '1.5 3', color: dark ? ERP.dark : ERP.light },
  ]
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs" aria-label="Legend">
      {groups.map((g) => (
        <span key={g} className="flex items-center gap-1.5">
          <span
            className={`inline-block size-3 border ${ROUNDED_GROUPS.has(g) ? 'rounded-[3px]' : 'rounded-full'}`}
            style={{ background: fill(g, dark), borderColor: stroke(g, dark) }}
          />
          {g}
        </span>
      ))}
      <span className="bg-border h-4 w-px" aria-hidden />
      {kinds.map((k) => (
        <span key={k.kind} className="flex items-center gap-1.5">
          <svg width="26" height="8" aria-hidden>
            <line x1="0" y1="4" x2="26" y2="4" stroke={k.color} strokeWidth="2.2" strokeDasharray={k.dash} strokeLinecap="round" />
          </svg>
          {kindLabel[k.kind]}
        </span>
      ))}
    </div>
  )
}

// ---- side panel

function Detail({ node, edge, byId }: { node?: GraphNode; edge?: GraphEdge; byId: Map<string, GraphNode> }) {
  if (node) {
    return (
      <div className="space-y-3 text-sm">
        <div>
          <h3 className="font-medium">{node.label}</h3>
          <p className="text-muted-foreground break-all text-xs">{node.id}</p>
        </div>
        <div className="flex flex-wrap gap-1">
          <Badge>{node.group}</Badge>
          {node.types.map((t) => (
            <Badge key={t} variant="outline" title={t}>{short(t)}</Badge>
          ))}
        </div>
        {node.properties.length === 0 ? (
          <p className="text-muted-foreground text-xs">No literal properties.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Property</TableHead>
                <TableHead>Value</TableHead>
                <TableHead>Type</TableHead>
                <TableHead>Origin</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {node.properties.map((p, i) => (
                <TableRow key={`${p.predicate}-${i}`}>
                  <TableCell className="whitespace-normal">{p.label}</TableCell>
                  <TableCell className="whitespace-normal break-all">{p.value}</TableCell>
                  <TableCell className="text-muted-foreground">{short(p.datatype)}</TableCell>
                  <TableCell className="whitespace-normal text-xs">{p.origin.kind}: {p.origin.source}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </div>
    )
  }
  if (edge) {
    const s = byId.get(edge.source)?.label ?? short(edge.source)
    const t = byId.get(edge.target)?.label ?? short(edge.target)
    return (
      <div className="space-y-3 text-sm">
        <div>
          <h3 className="font-medium">
            {s} <span className="text-muted-foreground">—{edge.label}→</span> {t}
          </h3>
          <p className="text-muted-foreground break-all text-xs">{edge.predicate}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={edge.origin.kind === 'inferred' ? 'secondary' : 'outline'}>{kindLabel[edge.origin.kind]}</Badge>
          <span>{originText(edge.origin)}</span>
        </div>
        {edge.sources.length > 0 && (
          <div>
            <div className="text-muted-foreground mb-1 text-xs">Sources</div>
            <ul className="list-disc space-y-0.5 pl-4 text-xs">
              {edge.sources.map((x) => <li key={x} className="break-all">{x}</li>)}
            </ul>
          </div>
        )}
      </div>
    )
  }
  return <p className="text-muted-foreground text-sm">Click a node or an edge to see what it is and where it comes from. Click a route on the left to highlight it.</p>
}

// ---- page

export default function GraphExplain() {
  const [params, setParams] = useSearchParams()
  const rawPairs = params.get('pairs')
  const pairs = useMemo(() => parsePairs(rawPairs), [rawPairs])
  const [maxHops, setMaxHops] = useState(7)
  const [from, setFrom] = useState({ iri: pairs[0].from, label: tail(pairs[0].from) })
  const [to, setTo] = useState({ iri: pairs[0].to, label: tail(pairs[0].to) })
  const [selection, setSelection] = useState<Selection>(null)
  const [route, setRoute] = useState<{ p: number; r: number } | null>(null)
  const apiRef = useRef<CanvasApi | null>(null)
  const dark = useIsDark()

  const req = useMemo(() => ({ pairs, max_depth: maxHops, max_paths: 3 }), [pairs, maxHops])
  const [submitted, setSubmitted] = useState(req)
  useEffect(() => setSubmitted((s) => (JSON.stringify(s.pairs) === JSON.stringify(pairs) ? s : { ...s, pairs })), [pairs])
  const explain = useQuery({ queryKey: ['graph-explain', submitted], queryFn: () => api.explainGraph(submitted), retry: false })
  const data = explain.data

  // Keep the pickers in step with the URL, and use real labels once known.
  useEffect(() => {
    setFrom({ iri: pairs[0].from, label: tail(pairs[0].from) })
    setTo({ iri: pairs[0].to, label: tail(pairs[0].to) })
  }, [pairs])
  useEffect(() => {
    const p = data?.paths[0]
    if (p && p.from === pairs[0].from && p.to === pairs[0].to) {
      setFrom({ iri: p.from, label: p.from_label })
      setTo({ iri: p.to, label: p.to_label })
    }
  }, [data, pairs])
  useEffect(() => {
    setSelection(null)
    setRoute(null)
  }, [data])

  const byId = useMemo(() => new Map((data?.nodes ?? []).map((n) => [n.id, n])), [data])
  const edgeById = useMemo(() => new Map((data?.edges ?? []).map((e) => [e.id, e])), [data])
  const groups = useMemo(() => {
    const present = new Set((data?.nodes ?? []).map((n) => groupKey(n.group)))
    return GROUPS.filter((g) => present.has(g))
  }, [data])

  const highlight = useMemo(() => (route && data ? data.paths[route.p]?.routes[route.r] ?? null : null), [route, data])

  const submit = () => {
    setParams({ pairs: serialise([{ from: from.iri, to: to.iri }]) })
    setSubmitted({ pairs: [{ from: from.iri, to: to.iri }], max_depth: maxHops, max_paths: 3 })
  }
  const removePair = (i: number) => setParams({ pairs: serialise(pairs.filter((_, j) => j !== i)) })
  const select = useCallback((s: Selection) => setSelection(s), [])

  const chain = (nodes: string[], edges: string[]) => (
    <span className="leading-relaxed">
      {nodes.map((id, i) => {
        const e = edgeById.get(edges[i])
        const forward = e ? e.source === id : true
        return (
          <span key={`${id}-${i}`}>
            <span className="font-medium">{byId.get(id)?.label ?? short(id)}</span>
            {i < edges.length && (
              <span className="text-muted-foreground">
                {' '}
                {forward ? `—${e?.label ?? '?'}→` : `←${e?.label ?? '?'}—`}{' '}
              </span>
            )}
          </span>
        )
      })}
    </span>
  )

  const sel = selection?.kind === 'node' ? { node: byId.get(selection.id) } : selection?.kind === 'edge' ? { edge: edgeById.get(selection.id) } : {}

  return (
    <div className="space-y-4">
      <PageHeader
        title="Explain graph"
        subtitle="Why a result happens: the chain of graph facts behind it, and where each fact comes from. Solid = asserted, dashed = inferred by a rule, dotted = ERP number."
      />

      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
      >
        <EntityPicker label="From" value={from} onPick={(e) => setFrom({ iri: e.iri, label: e.label })} />
        <EntityPicker label="To" value={to} onPick={(e) => setTo({ iri: e.iri, label: e.label })} />
        <Field label="Max hops">
          <input className={`${fieldClass} w-24`} type="number" min={1} max={20} value={maxHops} onChange={(e) => setMaxHops(Math.max(1, Number(e.target.value) || 1))} />
        </Field>
        <Button type="submit">Explain</Button>
      </form>

      {pairs.length > 1 && (
        <div className="flex flex-wrap items-center gap-2 text-xs" aria-label="Pairs being explained">
          <span className="text-muted-foreground">Pairs:</span>
          {pairs.map((p, i) => (
            <span key={`${p.from}|${p.to}`} className="bg-muted flex items-center gap-1 rounded-full py-0.5 pr-1 pl-2.5">
              {data?.paths.find((x) => x.from === p.from && x.to === p.to)?.from_label ?? tail(p.from)} to {data?.paths.find((x) => x.from === p.from && x.to === p.to)?.to_label ?? tail(p.to)}
              <button type="button" className="hover:bg-accent rounded-full px-1.5" aria-label={`Remove pair ${i + 1}`} onClick={() => removePair(i)}>
                ×
              </button>
            </span>
          ))}
        </div>
      )}

      <Loading query={explain} />

      {data && (
        <div className="grid gap-4 lg:grid-cols-[minmax(0,18rem)_minmax(0,1fr)_minmax(0,20rem)]">
          <section aria-label="Explained pairs and routes" className="space-y-3 text-sm lg:max-h-[640px] lg:overflow-auto">
            {data.paths.map((p, pi) => (
              <div key={`${p.from}|${p.to}`} className="rounded-lg border p-3">
                <div className="mb-2 flex flex-wrap items-center gap-2 font-medium">
                  {p.from_label} to {p.to_label}
                  {!p.found && <Badge variant="destructive">no connection found</Badge>}
                </div>
                <ol className="space-y-2">
                  {p.routes.map((r, ri) => {
                    const active = route?.p === pi && route.r === ri
                    return (
                      <li key={ri}>
                        <button
                          type="button"
                          aria-pressed={active}
                          className={`w-full rounded-md border p-2 text-left text-xs ${active ? 'border-primary bg-accent' : 'hover:bg-accent/60'}`}
                          onClick={() => setRoute(active ? null : { p: pi, r: ri })}
                        >
                          <span className="text-muted-foreground mb-0.5 block">Route {ri + 1}</span>
                          {chain(r.nodes, r.edges)}
                        </button>
                      </li>
                    )
                  })}
                </ol>
              </div>
            ))}
          </section>

          <section className="min-w-0 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <Legend groups={groups} dark={dark} />
              <div className="flex shrink-0 gap-1">
                <Button type="button" variant="outline" size="sm" onClick={() => apiRef.current?.fit()}>Fit</Button>
                <Button type="button" variant="outline" size="sm" aria-label="Zoom in" onClick={() => apiRef.current?.zoom(1.25)}>+</Button>
                <Button type="button" variant="outline" size="sm" aria-label="Zoom out" onClick={() => apiRef.current?.zoom(0.8)}>−</Button>
              </div>
            </div>
            <div role="region" aria-label={`Knowledge graph with ${data.nodes.length} nodes and ${data.edges.length} edges. The routes listed beside the graph give the same chains as text.`}>
              <GraphCanvas data={data} highlight={highlight} onSelect={select} apiRef={apiRef} />
            </div>
            {data.nodes.length === 0 && <p className="text-muted-foreground text-sm">Nothing to draw: no facts connect these entities.</p>}
          </section>

          <aside aria-label="Selected node or edge" className="rounded-lg border p-3 lg:max-h-[640px] lg:overflow-auto">
            <Detail {...sel} byId={byId} />
          </aside>
        </div>
      )}
    </div>
  )
}
