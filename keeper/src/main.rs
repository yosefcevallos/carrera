mod accounts;
mod alerts;
mod alt;
mod calendar;
mod chain;
mod config;
mod fast;
mod feed;
mod hourly;
mod ix;
mod lease;
mod prove;
mod rule;
mod settle;
mod status;
mod venue;
mod venue_accounts;
mod venue_setup;
mod venues;

use accounts::VaultState;
use alerts::Alerts;
use anyhow::Result;
use chain::Chain;
use clap::{Parser, Subcommand, ValueEnum};
use config::{Config, Feed};
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
    /// Shared HTTP client for Jupiter and Phoenix API calls.
    pub http: reqwest::Client,
    /// Phoenix exchange keys and markets, refreshed hourly (real build).
    pub phoenix_keys: Mutex<Option<venue::PhoenixKeys>>,
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
    /// Run the hourly, 60 s and settlement loops under the leader lease.
    Run {
        /// Where hourly inputs come from; overrides `feed` in the config.
        #[arg(long, value_enum)]
        feed: Option<FeedArg>,
        /// Feed funding, rates and prices from a JSON file (implies --feed mock).
        #[arg(long)]
        mock: Option<PathBuf>,
    },
    /// Run one pass and exit.
    Once {
        which: Pass,
        #[arg(long, value_enum)]
        feed: Option<FeedArg>,
        #[arg(long)]
        mock: Option<PathBuf>,
    },
    /// Print each vault's state, rule inputs and health.
    Status,
    /// Print a vault's 22-account Kamino block (fetched from chain) and its venue_data.
    KaminoBlock { symbol: String },
    /// Create a vault's Kamino user metadata and obligation (sends a transaction; keeper pays rent).
    InitObligation { symbol: String },
    /// Dry-run a Jupiter route for a vault: prints the block and data (no transaction).
    JupiterRoute {
        symbol: String,
        /// Amount in base units of the input mint (USDC when --to-stock, else the xStock).
        amount: u64,
        #[arg(long)]
        to_stock: bool,
    },
    /// Venue proofs (nothing is sent).
    #[command(subcommand)]
    Venues(VenuesCmd),
}

#[derive(Subcommand)]
enum VenuesCmd {
    /// Build the real-build wind_step(1) with a live Jupiter route, simulate it, and dump the
    /// route's accounts and programs as fork-test fixtures.
    ProveJupiter {
        #[arg(long)]
        vault: String,
        /// USDC base units to quote (the program patches the exact amount at execution).
        #[arg(long, default_value_t = 1_000_000)]
        amount: u64,
        /// Jupiter `dexes=` filter for the dumped routes; "any" lifts it. The fork can only replay
        /// AMMs whose state is not quote-time-bound (Whirlpool is).
        #[arg(long, default_value = "Whirlpool")]
        dexes: String,
        /// Fixture directory (default: ../program/tests/fixtures/jupiter).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Simulate the vault's Phoenix trader registration and the wind_step(3) shape, and dump the
    /// Phoenix / Ember accounts as fork-test fixtures.
    ProvePhoenix {
        #[arg(long)]
        vault: String,
        /// Fixture directory (default: ../program/tests/fixtures/phoenix).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Pass {
    Hourly,
    Fast,
    Settle,
}

#[derive(Clone, Copy, ValueEnum)]
enum FeedArg {
    /// Program reads Hawkeye / Kamino / oracle accounts itself (non-mock program build).
    Onchain,
    /// Keeper fetches Phoenix, Kamino and Jupiter public APIs (mock-venues program build).
    Live,
    /// Keeper reads a JSON file (mock-venues program build, localnet).
    Mock,
}

fn venues_for(cfg: &Config, feed: Option<FeedArg>, mock: Option<PathBuf>) -> Result<Venues> {
    let feed = match (feed, &mock) {
        (_, Some(_)) => Feed::Mock,
        (Some(FeedArg::Onchain), _) => Feed::Onchain,
        (Some(FeedArg::Live), _) => Feed::Live,
        (Some(FeedArg::Mock), _) => Feed::Mock,
        (None, None) => cfg.feed,
    };
    Ok(match feed {
        Feed::Onchain => Venues::onchain(),
        Feed::Mock => {
            let path = mock.or_else(|| cfg.mock_path.clone()).ok_or_else(|| anyhow::anyhow!("feed = mock needs --mock <path> or mock_path in config"))?;
            Venues::mock(path)?
        }
        Feed::Live => Venues::live(feed::LiveFeed::new(
            feed::FeedUrls {
                phoenix: cfg.phoenix_api_url.clone(),
                kamino: cfg.kamino_api_url.clone(),
                kamino_market: cfg.kamino_market.clone(),
                jupiter: cfg.jupiter_price_url.clone(),
            },
            cfg.vaults.iter().map(|v| feed::FeedVault { symbol: v.symbol.clone(), mint: v.mint.to_string(), phoenix_market: v.phoenix_market.clone() }).collect(),
        )),
    })
}

fn ctx(cfg: Config, venues: Venues) -> Result<Arc<Ctx>> {
    let chain = Chain::new(&cfg.rpc_url, &cfg.keypair_path, cfg.program_id)?;
    let alerts = Alerts::new(cfg.alert_webhook_url.clone());
    let mut st = status::StatusState { history_path: cfg.history_path.clone(), ..Default::default() };
    st.keeper.instance_id = instance_id();
    st.keeper.program_id = cfg.program_id.to_string();
    st.keeper.cluster = cfg.rpc_url.clone();
    Ok(Arc::new(Ctx {
        cfg,
        chain,
        venues: Mutex::new(venues),
        alerts: Mutex::new(alerts),
        status: RwLock::new(st),
        http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?,
        phoenix_keys: Mutex::new(None),
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cli = Cli::parse();
    let cfg = Config::load(cli.config)?;

    match cli.cmd {
        Cmd::Status => status(ctx(cfg, Venues::onchain())?).await,
        Cmd::KaminoBlock { symbol } => {
            let c = ctx(cfg, Venues::onchain())?;
            venue_setup::print_kamino_block(&c.chain, &c.cfg, &symbol).await
        }
        Cmd::InitObligation { symbol } => {
            let c = ctx(cfg, Venues::onchain())?;
            venue_setup::init_obligation(&c.chain, &c.cfg, &symbol).await
        }
        Cmd::JupiterRoute { symbol, amount, to_stock } => {
            let c = ctx(cfg, Venues::onchain())?;
            venue_setup::print_jupiter_route(&c.chain, &c.cfg, &symbol, amount, to_stock).await
        }
        Cmd::Venues(VenuesCmd::ProveJupiter { vault, amount, dexes, out }) => {
            let c = ctx(cfg, Venues::onchain())?;
            let out = out.unwrap_or_else(|| PathBuf::from("../program/tests/fixtures/jupiter"));
            let dexes = if dexes.eq_ignore_ascii_case("any") { None } else { Some(dexes.as_str()) };
            prove::prove_jupiter(&c, &vault, amount, dexes, &out).await
        }
        Cmd::Venues(VenuesCmd::ProvePhoenix { vault, out }) => {
            let c = ctx(cfg, Venues::onchain())?;
            let out = out.unwrap_or_else(|| PathBuf::from("../program/tests/fixtures/phoenix"));
            prove::prove_phoenix(&c, &vault, &out).await
        }
        Cmd::Once { which, feed, mock } => {
            let v = venues_for(&cfg, feed, mock)?;
            let c = ctx(cfg, v)?;
            match which {
                Pass::Hourly => hourly::run_once(&c).await,
                Pass::Fast => fast::run_once(&c).await,
                Pass::Settle => settle::run_once(&c).await,
            }
        }
        Cmd::Run { feed, mock } => {
            let v = venues_for(&cfg, feed, mock)?;
            run(ctx(cfg, v)?).await
        }
    }
}

async fn run(c: Arc<Ctx>) -> Result<()> {
    let holder = instance_id();
    let lease = FileLease::new(&c.cfg.lease_path, holder, Duration::from_secs(c.cfg.lease_ttl_secs));
    tracing::info!(
        keeper = %c.chain.keeper(), program = %c.cfg.program_id, feed = c.venues.lock().await.name(),
        lease = %c.cfg.lease_path.display(), "starting"
    );
    let server = tokio::spawn(status::serve(c.clone()));

    let mut hourly = tokio::time::interval(Duration::from_secs(c.cfg.hourly_interval_secs));
    let mut fast = tokio::time::interval(Duration::from_secs(c.cfg.fast_interval_secs));
    let mut settle = tokio::time::interval(Duration::from_secs(c.cfg.settle_interval_secs));
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
            _ = settle.tick() => {
                if leader {
                    if let Err(e) = settle::run_once(&c).await { tracing::error!("settle pass failed: {e:#}"); }
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
