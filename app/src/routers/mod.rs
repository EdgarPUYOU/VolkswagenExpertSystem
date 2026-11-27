//! HTTP layer. Handlers call services and serialise; no business logic here.
use crate::{
    kg::Kg,
    services::{
        chat::{self, ChatRequest},
        finance::{self, BadRequest, ScenarioRequest, Snapshot},
        market::{self, MarketData},
        portfolio, questions,
    },
    web,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub kg: Arc<Kg>,
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub graphs: Arc<Vec<crate::kg::GraphStat>>,
}

type ApiResult = Result<Json<Value>, (StatusCode, String)>;

fn internal(e: impl std::fmt::Display) -> (StatusCode, String) {
    tracing::error!(error = %e, "request failed");
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Caller mistakes (unknown IRI, bad number) are 400; anything else is 500.
fn fail(e: anyhow::Error) -> (StatusCode, String) {
    match e.downcast_ref::<BadRequest>() {
        Some(b) => (StatusCode::BAD_REQUEST, b.0.clone()),
        None => internal(format!("{e:#}")),
    }
}

fn snapshot(s: &AppState) -> anyhow::Result<Snapshot> {
    let conn = s.db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    Snapshot::load(&s.kg, &conn)
}

fn market_data(s: &AppState) -> anyhow::Result<(Snapshot, MarketData)> {
    let conn = s.db.lock().map_err(|e| anyhow::anyhow!("db lock poisoned: {e}"))?;
    Ok((Snapshot::load(&s.kg, &conn)?, MarketData::load(&s.kg, &conn)?))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/kg/graphs", get(graphs))
        .route("/api/models", get(models))
        .route("/api/contracts", get(contracts))
        .route("/api/scenarios/run", post(run_scenario))
        .route("/api/scenarios", get(list_scenarios))
        .route("/api/scenarios/{id}", get(get_scenario))
        .route("/api/chat", post(chat))
        .route("/api/chat/status", get(chat_status))
        .route("/api/chat/tools", get(chat_tools))
        .route("/api/chat/tools/{name}", post(chat_tool))
        .route("/api/dashboard", get(dashboard))
        .route("/api/market/series", get(market_series))
        .route("/api/exposure/currency", get(currency_exposure))
        .route("/api/exposure/commodity", get(commodity_exposure))
        .route("/api/contracts/savings", get(savings))
        .route("/api/contracts/expiring", get(expiring))
        .route("/api/suppliers/risk", get(supplier_risk))
        .route("/api/suppliers/lead-time", get(lead_time))
        .route("/api/components/single-sourced", get(single_sourced))
        .route("/api/models/{model}/margin-trace", get(margin_trace))
        .route("/api/strategy/subsidy", get(subsidy))
        .route("/api/strategy/co2", get(co2))
        .route("/api/strategy/competitor", get(competitor))
        .route("/api/strategy/sites", get(sites))
        .route("/api/strategy/mix", get(mix))
        .route("/api/strategy/markets", get(strategy_markets))
        .fallback(web::handler)
        .with_state(state)
}

async fn health(State(s): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let sqlite = s
        .db
        .lock()
        .map_err(|e| e.to_string())
        .and_then(|c| c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string()));
    let kg = s.kg.len().map_err(|e| e.to_string());
    let body = json!({
        "sqlite": sqlite.as_ref().map(|_| "ok").unwrap_or("error"),
        "kg_triples": kg.as_ref().ok(),
    });
    if sqlite.is_ok() && kg.is_ok() {
        Ok(Json(body))
    } else {
        Err((StatusCode::SERVICE_UNAVAILABLE, Json(body)))
    }
}

async fn graphs(State(s): State<AppState>) -> Json<Value> {
    Json(json!(*s.graphs))
}

async fn models(State(s): State<AppState>) -> ApiResult {
    let rows = portfolio::models(&s.kg).map_err(internal)?;
    Ok(Json(json!(rows)))
}

async fn contracts(State(s): State<AppState>) -> ApiResult {
    let rows = portfolio::contracts(&s.kg).map_err(internal)?;
    Ok(Json(json!(rows)))
}

async fn run_scenario(State(s): State<AppState>, Json(req): Json<ScenarioRequest>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    let result = finance::run_scenario(&snap, &req).map_err(fail)?;
    let id = match req.save_as.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => {
            let params = serde_json::to_string(&req.shocks).map_err(internal)?;
            let body = serde_json::to_string(&result).map_err(internal)?;
            let conn = s.db.lock().map_err(internal)?;
            conn.execute(
                "INSERT INTO scenario_runs (name, params_json, result_json) VALUES (?1, ?2, ?3)",
                rusqlite::params![name, params, body],
            )
            .map_err(internal)?;
            Some(conn.last_insert_rowid())
        }
        None => None,
    };
    Ok(Json(json!({ "id": id, "result": result })))
}

async fn list_scenarios(State(s): State<AppState>) -> ApiResult {
    let conn = s.db.lock().map_err(internal)?;
    let mut stmt = conn
        .prepare("SELECT id, name, params_json, created_at FROM scenario_runs ORDER BY id DESC")
        .map_err(internal)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "shocks": serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(Value::Null),
                "created_at": r.get::<_, String>(3)?,
            }))
        })
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    Ok(Json(json!(rows)))
}

async fn get_scenario(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let conn = s.db.lock().map_err(internal)?;
    let row = conn.query_row(
        "SELECT name, result_json, created_at FROM scenario_runs WHERE id = ?1",
        [id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
    );
    match row {
        Ok((name, result, created_at)) => Ok(Json(json!({
            "id": id,
            "name": name,
            "created_at": created_at,
            "result": serde_json::from_str::<Value>(&result).map_err(internal)?,
        }))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err((StatusCode::NOT_FOUND, format!("no scenario run {id}"))),
        Err(e) => Err(internal(e)),
    }
}

async fn market_series(State(s): State<AppState>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(finance::series_overview(&snap))))
}

async fn currency_exposure(State(s): State<AppState>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::currency_exposure(&snap).map_err(fail)?)))
}

#[derive(Deserialize)]
struct BumpParams {
    /// Size of the index move in percent, default 10
    bump: Option<f64>,
}

async fn commodity_exposure(State(s): State<AppState>, Query(p): Query<BumpParams>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::commodity_sensitivity(&snap, p.bump.unwrap_or(10.0)).map_err(fail)?)))
}

async fn savings(State(s): State<AppState>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::discount_savings(&snap))))
}

#[derive(Deserialize)]
struct ExpiringParams {
    months: Option<u32>,
    /// YYYY-MM-DD, defaults to today
    today: Option<String>,
}

async fn expiring(State(s): State<AppState>, Query(p): Query<ExpiringParams>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    let today = p.today.unwrap_or_else(today);
    Ok(Json(json!(questions::expiring(&snap, &today, p.months.unwrap_or(12)).map_err(fail)?)))
}

async fn supplier_risk(State(s): State<AppState>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::supplier_risk(&snap).map_err(fail)?)))
}

#[derive(Deserialize)]
struct LeadTimeParams {
    supplier: String,
    /// Multiplier on the supplier's lead time, default 2
    factor: Option<f64>,
}

async fn lead_time(State(s): State<AppState>, Query(p): Query<LeadTimeParams>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::lead_time_impact(&snap, &p.supplier, p.factor.unwrap_or(2.0)).map_err(fail)?)))
}

async fn single_sourced(State(s): State<AppState>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::single_sourced(&snap))))
}

#[derive(Deserialize)]
struct TraceParams {
    from: Option<String>,
    to: Option<String>,
}

async fn margin_trace(State(s): State<AppState>, Path(model): Path<String>, Query(p): Query<TraceParams>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::margin_trace(&snap, &model, p.from.as_deref(), p.to.as_deref()).map_err(fail)?)))
}

async fn dashboard(State(s): State<AppState>) -> ApiResult {
    let snap = snapshot(&s).map_err(fail)?;
    Ok(Json(json!(questions::dashboard(&snap, &today()).map_err(fail)?)))
}

async fn chat(State(s): State<AppState>, Json(req): Json<ChatRequest>) -> ApiResult {
    let (snap, md) = market_data(&s).map_err(fail)?;
    Ok(Json(json!(chat::plan(&snap, &md, &req.message, &today()).map_err(fail)?)))
}

async fn chat_status() -> Json<Value> {
    Json(json!({ "planner": "rules", "llm": false, "suggestions": chat::SUGGESTIONS }))
}

async fn chat_tools() -> Json<Value> {
    Json(json!(chat::tools()))
}

async fn chat_tool(State(s): State<AppState>, Path(name): Path<String>, Json(args): Json<Value>) -> ApiResult {
    let (snap, md) = market_data(&s).map_err(fail)?;
    Ok(Json(json!(chat::call_tool(&snap, &md, &name, &args, &today()).map_err(fail)?)))
}

#[derive(Deserialize)]
struct SubsidyParams {
    /// Market name or IRI, default Germany
    market: Option<String>,
    elasticity: Option<f64>,
}

async fn subsidy(State(s): State<AppState>, Query(p): Query<SubsidyParams>) -> ApiResult {
    let (snap, md) = market_data(&s).map_err(fail)?;
    let r = market::subsidy_removal(&snap, &md, p.market.as_deref().unwrap_or("Germany"), p.elasticity.unwrap_or(market::DEFAULT_SUBSIDY_ELASTICITY));
    Ok(Json(json!(r.map_err(fail)?)))
}

#[derive(Deserialize)]
struct Co2Params {
    /// Percent the target is lowered by, default 10
    tighten: Option<f64>,
}

async fn co2(State(s): State<AppState>, Query(p): Query<Co2Params>) -> ApiResult {
    let (snap, md) = market_data(&s).map_err(fail)?;
    Ok(Json(json!(market::co2_fleet(&snap, &md, p.tighten.unwrap_or(10.0)).map_err(fail)?)))
}

#[derive(Deserialize)]
struct CompetitorParams {
    competitor: Option<String>,
    cut: Option<f64>,
    elasticity: Option<f64>,
}

async fn competitor(State(s): State<AppState>, Query(p): Query<CompetitorParams>) -> ApiResult {
    let (snap, md) = market_data(&s).map_err(fail)?;
    let r = market::competitor_cut(&snap, &md, p.competitor.as_deref(), p.cut.unwrap_or(10.0), p.elasticity.unwrap_or(market::DEFAULT_CROSS_ELASTICITY));
    Ok(Json(json!(r.map_err(fail)?)))
}

async fn sites(State(s): State<AppState>) -> ApiResult {
    let (snap, md) = market_data(&s).map_err(fail)?;
    Ok(Json(json!(market::site_dependency(&snap, &md))))
}

#[derive(Deserialize)]
struct MixParams {
    /// Percent of combustion cars moved to electric, default 10
    shift: Option<f64>,
}

async fn mix(State(s): State<AppState>, Query(p): Query<MixParams>) -> ApiResult {
    let (snap, md) = market_data(&s).map_err(fail)?;
    Ok(Json(json!(market::mix_analysis(&snap, &md, p.shift.unwrap_or(10.0)).map_err(fail)?)))
}

/// Reference lists for the Markets and regulation page: markets, competitors and the regulations in force.
async fn strategy_markets(State(s): State<AppState>) -> ApiResult {
    let (_, md) = market_data(&s).map_err(fail)?;
    Ok(Json(json!({
        "markets": md.markets.values().collect::<Vec<_>>(),
        "subsidies": md.subsidies,
        "co2": md.co2,
        "competitors": md.rivals.values().map(|r| r.competitor.clone()).collect::<std::collections::BTreeSet<_>>(),
    })))
}
