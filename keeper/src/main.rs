mod accounts;
mod alerts;
mod calendar;
mod chain;
mod config;
mod fast;
mod hourly;
mod ix;
mod lease;
mod rule;
mod status;
mod venues;

use accounts::VaultState;
use alerts::Alerts;
use anyhow::Result;
use chain::Chain;
use clap::{Parser, Subcommand, ValueEnum};
use config::Config;
use lease::FileLease;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock};
use venues::Venues;

/// Shared state for the loops.
pub struct Ctx {
    pub cfg: Config,
    pub chain: Chain,
    pub venues: Mutex<Venues>,
    pub alerts: Mutex<Alerts>,
    pub status: RwLock<status::StatusState>,
}

#[derive(Parser)]
#[command(name = "carrera-keeper", about = "Off-chain crank for the carrera_overlay program")]
struct Cli {
    /// Config file (default: $CARRERA_KEEPER_CONFIG or ./keeper.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the hourly and 60 s loops under the leader lease.
    Run {
        /// Feed funding, rates and prices from a JSON file (program must be built with mock-venues).
        #[arg(long)]
        mock: Option<PathBuf>,
    },
    /// Run one pass and exit.
    Once {
        which: Pass,
        #[arg(long)]
        mock: Option<PathBuf>,
    },
    /// Print each vault's state, rule inputs and health.
    Status,
}

#[derive(Clone, Copy, ValueEnum)]
enum Pass {
    Hourly,
    Fast,
}

fn ctx(cfg: Config, mock: Option<PathBuf>) -> Result<Arc<Ctx>> {
    let chain = Chain::new(&cfg.rpc_url, &cfg.keypair_path, cfg.program_id)?;
    let venues = match mock {
        Some(p) => Venues::mock(p)?,
        None => Venues::live(),
    };
    let alerts = Alerts::new(cfg.alert_webhook_url.clone());
    let mut st = status::StatusState { history_path: cfg.history_path.clone(), ..Default::default() };
    st.keeper.instance_id = instance_id();
    st.keeper.program_id = cfg.program_id.to_string();
    st.keeper.cluster = cfg.rpc_url.clone();
    Ok(Arc::new(Ctx { cfg, chain, venues: Mutex::new(venues), alerts: Mutex::new(alerts), status: RwLock::new(st) }))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cli = Cli::parse();
    let cfg = Config::load(cli.config)?;

    match cli.cmd {
        Cmd::Status => status(ctx(cfg, None)?).await,
        Cmd::Once { which, mock } => {
            let c = ctx(cfg, mock)?;
            match which {
                Pass::Hourly => hourly::run_once(&c).await,
                Pass::Fast => fast::run_once(&c).await,
            }
        }
        Cmd::Run { mock } => run(ctx(cfg, mock)?).await,
    }
}

async fn run(c: Arc<Ctx>) -> Result<()> {
    let holder = instance_id();
    let lease = FileLease::new(&c.cfg.lease_path, holder, Duration::from_secs(c.cfg.lease_ttl_secs));
    tracing::info!(
        keeper = %c.chain.keeper(), program = %c.cfg.program_id, mock = c.venues.lock().await.is_mock(),
        lease = %c.cfg.lease_path.display(), "starting"
    );
    let server = tokio::spawn(status::serve(c.clone()));

    let mut hourly = tokio::time::interval(Duration::from_secs(c.cfg.hourly_interval_secs));
    let mut fast = tokio::time::interval(Duration::from_secs(c.cfg.fast_interval_secs));
    let mut renew = tokio::time::interval(Duration::from_secs((c.cfg.lease_ttl_secs / 3).max(5)));
    let mut leader = false;

    loop {
        tokio::select! {
            _ = renew.tick() => {
                let now = lease.try_acquire().unwrap_or(false);
                if now != leader {
                    tracing::info!(holder = lease.holder(), "leader = {now}");
                    leader = now;
                    c.status.write().await.keeper.is_leader = now;
                }
            }
            _ = hourly.tick() => {
                if leader {
                    if let Err(e) = hourly::run_once(&c).await { tracing::error!("hourly pass failed: {e:#}"); }
                }
            }
            _ = fast.tick() => {
                if leader {
                    if let Err(e) = fast::run_once(&c).await { tracing::error!("fast pass failed: {e:#}"); }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                server.abort();
                lease.release();
                tracing::info!("stopped");
                return Ok(());
            }
        }
    }
}

async fn status(c: Arc<Ctx>) -> Result<()> {
    let chain = &c.chain;
    let reg = chain.registry().await?;
    let slot = chain.slot().await?;
    println!(
        "registry: paused={} borrow={}bps supply={}bps rates_slot={} keeper={} sol={:.3}",
        reg.paused, reg.borrow_apy_bps, reg.supply_apy_bps, reg.rates_slot, chain.keeper(), chain.sol_balance().await.unwrap_or(0.0)
    );
    println!("{:<7} {:<10} {:>4} {:>8} {:>8} {:>8} {:>6} {:>7} {:>6} {:>14} {:>10} {:>5}",
        "vault", "state", "step", "f_avg", "hurdle_p", "hurdle_i", "ltv", "margin", "open", "nav_usd", "share_px", "nav_age");
    for vc in &c.cfg.vaults {
        match chain.vault(&vc.mint).await {
            Ok(v) => {
                let h = rule::hurdles(&v.params, reg.supply_apy_bps, reg.borrow_apy_bps);
                let state = v.state().map(VaultState::name).unwrap_or("?");
                let margin = v.margin_bps(vc.stock_decimals).map(|m| m.to_string()).unwrap_or_else(|| "-".into());
                println!("{:<7} {:<10} {:>4} {:>8} {:>8} {:>8} {:>6} {:>7} {:>6} {:>14.2} {:>10.6} {:>5}",
                    vc.symbol, state, v.step, v.f_avg_bps(), h.from_parked_bps, h.from_idle_bps,
                    v.ltv_bps(vc.stock_decimals), margin, v.market_open,
                    v.nav_usd_e6 as f64 / 1e6, v.share_price_stock_e6 as f64 / 1e6, slot.saturating_sub(v.nav_slot));
            }
            Err(e) => println!("{:<7} {e:#}", vc.symbol),
        }
    }
    Ok(())
}

fn instance_id() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "local".into());
    format!("{}@{host}", std::process::id())
}
