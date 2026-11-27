// Thin client for the Rust API. All pages go through here.
export type Row = Record<string, string>

export interface GraphStat {
  graph: string
  file: string
  triples: number
}

export interface Health {
  sqlite: string
  kg_triples: number | null
}

// ---- scenarios

export interface ShockSpec {
  kind: 'commodity' | 'fx'
  target: string
  percent: number
}

export interface ScenarioRequest {
  shocks: ShockSpec[]
  margin_floor?: number
  save_as?: string
}

export interface ModelImpact {
  iri: string
  label: string
  brand: string
  profit_center: string
  price: number
  planned: number
  material_before: number
  material_after: number
  overhead: number
  margin_before_pct: number
  margin_after_pct: number
  extra_cost: number
  below_floor: boolean
  crossed_floor: boolean
}

export interface ProfitCenterImpact {
  iri: string
  label: string
  revenue: number
  extra_cost: number
  extra_cost_pct_of_revenue: number
  material_budget: number
  extra_cost_pct_of_budget: number
  margin_before_pct: number
  margin_after_pct: number
}

export interface ContractImpact {
  iri: string
  label: string
  supplier: string
  price_before: number
  price_after: number
  annual_units: number
  extra_cost: number
  sourcing: string
  end: string
  drivers: { shock: string; extra_cost: number }[]
}

export interface ScenarioResult {
  asof: string
  margin_floor_pct: number
  shocks: ShockSpec[]
  totals: {
    revenue: number
    extra_cost: number
    margin_before_pct: number
    margin_after_pct: number
    models_below_floor: number
    models_crossing_floor: number
  }
  by_profit_center: ProfitCenterImpact[]
  by_model: ModelImpact[]
  by_contract: ContractImpact[]
}

export interface SavedScenario {
  id: number
  name: string
  shocks: ShockSpec[]
  created_at: string
}

// ---- market and questions

export interface Series {
  iri: string
  label: string
  kind: 'index' | 'fx'
  unit: string
  latest: number
  change_6m_pct: number
  change_since_start_pct: number
  points: [string, number][]
}

export interface CurrencyExposure {
  asof: string
  total_spend_eur: number
  foreign_share_pct: number
  by_currency: { currency: string; code: string; spend_eur: number; share_pct: number; contracts: number; foreign: boolean }[]
  contracts: { iri: string; label: string; supplier: string; currency: string; spend_eur: number }[]
}

export interface Sensitivity {
  bump_pct: number
  indices: string[]
  rows: {
    profit_center: string
    label: string
    revenue: number
    linked_material_share_pct: number
    per_index: Record<string, number>
    all_indices_extra_cost: number
    all_indices_pct_of_revenue: number
  }[]
}

export interface Savings {
  total_annual_saving: number
  rows: {
    iri: string
    label: string
    supplier: string
    component: string
    projected: number
    tier: number
    current_price: number
    discount_price: number
    annual_saving: number
  }[]
}

export interface ExpiringContract {
  iri: string
  label: string
  supplier: string
  component: string
  end: string
  days_left: number
  committed: number
  projected: number
  under_committed: boolean
  annual_spend: number
  sourcing: string
  alternatives: string[]
}

export interface Expiring {
  today: string
  months: number
  cutoff: string
  contracts: ExpiringContract[]
}

export interface SupplierRisk {
  iri: string
  label: string
  country: string
  lead_time_days: number
  contracts: number
  components: string[]
  models: string[]
  revenue_at_risk: number
  revenue_share_pct: number
  annual_spend: number
  single_sourced_components: number
  alternatives: string[]
}

export interface SingleSource {
  component: string
  label: string
  contract: string
  supplier: string
  lead_time_days: number
  contract_end: string
  models: string[]
  revenue_dependent: number
}

export interface LeadTimeImpact {
  supplier: string
  label: string
  lead_time_days: number
  new_lead_time_days: number
  delay_days: number
  total_revenue_at_risk: number
  leads_affected: number
  models: {
    iri: string
    label: string
    via_components: string[]
    planned_launch: string | null
    new_launch: string | null
    revenue_at_risk: number
    high_intent_leads: string[]
  }[]
}

export interface MarginTrace {
  model: string
  label: string
  from: string
  to: string
  price: number
  margin_from_pct: number
  margin_to_pct: number
  change_pp: number
  material_change_eur_per_unit: number
  components: {
    component: string
    contract: string
    supplier: string
    currency: string
    price_from: number
    price_to: number
    delta_eur_per_unit: number
    drivers: { index: string; weight: number; pass_through: number; delta_eur_per_unit: number }[]
    fx_effect_eur_per_unit: number
    path: string[]
  }[]
  series: { month: string; margin_pct: number }[]
}

export interface Dashboard {
  asof: string
  planned_revenue: number
  contribution_margin_pct: number
  material_spend: number
  foreign_currency_share_pct: number
  commodity_linked_share_pct: number
  contracts: number
  contracts_under_committed: number
  contracts_expiring_12m: number
  single_sourced_components: number
  movers: { label: string; kind: string; change_6m_pct: number }[]
  margin_by_profit_center: [string, number][]
}

// ---- chat

export type ChatBlock =
  | { type: 'kpis'; items: { label: string; value: string; hint?: string; tone?: string }[] }
  | { type: 'table'; title: string; columns: string[]; rows: string[][] }
  | { type: 'bars'; title: string; unit: string; items: { label: string; value: number }[] }
  | { type: 'link'; label: string; to: string; shocks?: ShockSpec[] }

export interface ChatReply {
  text: string
  blocks: ChatBlock[]
  tool_calls: { name: string; args: unknown }[]
  planner: string
}

export interface ChatStatus {
  planner: string
  llm: boolean
  suggestions: string[]
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init)
  if (!res.ok) throw new Error((await res.text()) || `${path}: ${res.status}`)
  return res.json() as Promise<T>
}

const get = <T>(path: string) => request<T>(path)
// ---- strategy (Q10 to Q14)

export interface SubsidyRemoval {
  market: string
  subsidies: string[]
  elasticity: number
  lost_units: number
  lost_revenue: number
  lost_contribution: number
  models: {
    iri: string
    label: string
    brand: string
    subsidy_eur: number
    price: number
    units_in_market: number
    demand_loss_pct: number
    lost_units: number
    lost_revenue: number
    lost_contribution: number
  }[]
  leads: { customer: string; model: string }[]
}

export interface FleetRow {
  label: string
  cars_in_target_markets: number
  avg_g_per_km: number
  target_now: number
  penalty_now: number
  target_new: number
  penalty_new: number
  breaches_new: boolean
}

export interface Co2Fleet {
  regulation: string
  tighten_pct: number
  target_now: number
  target_new: number
  penalty_per_gram: number
  brands: FleetRow[]
  pooled: FleetRow
}

export interface CompetitorCut {
  competitor: string
  cut_pct: number
  elasticity: number
  revenue_exposed: number
  revenue_at_risk: number
  contribution_at_risk: number
  hits: {
    model: string
    brand: string
    rival: string
    competitor: string
    our_price: number
    rival_price_before: number
    rival_price_after: number
    undercut: boolean
    revenue_exposed: number
    volume_loss_pct: number
    revenue_at_risk: number
    contribution_at_risk: number
  }[]
}

export interface GroupSites {
  group: string
  revenue: number
  top_site: string
  top_share_pct: number
  sites: { site: string; brand: string; revenue: number; share_pct: number }[]
}

export interface MixAnalysis {
  shift_pct: number
  powertrains: {
    powertrain: string
    models: number
    units: number
    revenue: number
    contribution: number
    margin_pct: number
    contribution_per_unit: number
    subsidy_dependent: string[]
    co2_penalty_risk: string[]
  }[]
  shifts: {
    brand: string
    moved_units: number
    contribution_change: number
    avg_g_before: number
    avg_g_after: number
    co2_penalty_before: number
    co2_penalty_after: number
    net_change: number
  }[]
}

export interface StrategyRefs {
  markets: { iri: string; label: string; country: string }[]
  competitors: string[]
}

const q = (params: Record<string, string | number | undefined>) => {
  const s = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '') s.set(k, String(v))
  const str = s.toString()
  return str ? `?${str}` : ''
}

export const api = {
  health: () => get<Health>('/health'),
  graphs: () => get<GraphStat[]>('/api/kg/graphs'),
  models: () => get<Row[]>('/api/models'),
  contracts: () => get<Row[]>('/api/contracts'),
  dashboard: () => get<Dashboard>('/api/dashboard'),
  series: () => get<Series[]>('/api/market/series'),
  runScenario: (req: ScenarioRequest) =>
    request<{ id: number | null; result: ScenarioResult }>('/api/scenarios/run', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(req),
    }),
  savedScenarios: () => get<SavedScenario[]>('/api/scenarios'),
  chat: (message: string) =>
    request<ChatReply>('/api/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ message }),
    }),
  chatStatus: () => get<ChatStatus>('/api/chat/status'),
  savedScenario: (id: number) => get<{ id: number; name: string; result: ScenarioResult }>(`/api/scenarios/${id}`),
  currencyExposure: () => get<CurrencyExposure>('/api/exposure/currency'),
  sensitivity: (bump: number) => get<Sensitivity>(`/api/exposure/commodity${q({ bump })}`),
  savings: () => get<Savings>('/api/contracts/savings'),
  expiring: (months: number) => get<Expiring>(`/api/contracts/expiring${q({ months })}`),
  supplierRisk: () => get<SupplierRisk[]>('/api/suppliers/risk'),
  singleSourced: () => get<SingleSource[]>('/api/components/single-sourced'),
  leadTime: (supplier: string, factor: number) => get<LeadTimeImpact>(`/api/suppliers/lead-time${q({ supplier, factor })}`),
  strategyRefs: () => get<StrategyRefs>('/api/strategy/markets'),
  subsidy: (market: string, elasticity: number) => get<SubsidyRemoval>(`/api/strategy/subsidy${q({ market, elasticity })}`),
  co2: (tighten: number) => get<Co2Fleet>(`/api/strategy/co2${q({ tighten })}`),
  competitor: (competitor: string, cut: number, elasticity: number) =>
    get<CompetitorCut>(`/api/strategy/competitor${q({ competitor, cut, elasticity })}`),
  sites: () => get<GroupSites[]>('/api/strategy/sites'),
  mix: (shift: number) => get<MixAnalysis>(`/api/strategy/mix${q({ shift })}`),
  marginTrace: (model: string, from?: string, to?: string) =>
    get<MarginTrace>(`/api/models/${encodeURIComponent(model)}/margin-trace${q({ from, to })}`),
}

export const shortName = (iri: string) => iri.split('/').pop() ?? iri
export const num = (v: string | undefined) => (v === undefined ? NaN : Number(v))
export const fmt = new Intl.NumberFormat('en-US')
export const eur = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'EUR', maximumFractionDigits: 0 })
export const eurCompact = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'EUR',
  notation: 'compact',
  maximumFractionDigits: 1,
})
export const pct = (v: number, digits = 1) => `${v.toFixed(digits)}%`
export const signed = (v: number, digits = 1) => `${v > 0 ? '+' : ''}${v.toFixed(digits)}`
