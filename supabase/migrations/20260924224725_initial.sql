-- Carrera indexer schema. Written by the indexer service (service_role); read by the web app (anon).
-- Times are timestamptz; on-chain amounts are bigint in base units; *_e6 values carry 6 decimals.

-- ---------------------------------------------------------------------------
-- Reference
-- ---------------------------------------------------------------------------
create table public.vaults (
  symbol        text primary key,
  xstock_mint   text not null,
  vault_pubkey  text not null,
  tier          smallint not null check (tier between 0 and 3),
  share_mint    text not null
);
comment on table public.vaults is 'One row per overlay vault. Mints and PDAs are filled from the deployed program.';

-- ---------------------------------------------------------------------------
-- Raw event log (append-only, idempotent on signature + log position)
-- ---------------------------------------------------------------------------
create table public.program_events (
  id            bigint generated always as identity primary key,
  slot          bigint not null,
  signature     text not null,
  ix_index      integer not null,
  log_index       integer not null,
  event_name    text not null,
  vault_symbol  text references public.vaults (symbol),
  payload       jsonb not null,
  block_time    timestamptz not null,
  unique (signature, log_index)
);
create index program_events_slot_idx on public.program_events (slot);
create index program_events_vault_time_idx on public.program_events (vault_symbol, block_time desc);

-- ---------------------------------------------------------------------------
-- Time series derived from events
-- ---------------------------------------------------------------------------
create table public.nav_samples (
  vault_symbol          text not null references public.vaults (symbol),
  ts                    timestamptz not null,
  slot                  bigint not null,
  nav_usd_e6            bigint not null,
  share_price_stock_e6  bigint not null,
  price_e6              bigint,            -- made non-null in a later migration once the event carries it
  primary key (vault_symbol, ts)
);

create table public.rule_samples (
  vault_symbol    text not null references public.vaults (symbol),
  ts              timestamptz not null,
  f_avg_bps       bigint not null,
  parked_apy_bps  integer not null,
  r_bps           integer not null,
  hurdle_bps      bigint not null,
  decision        smallint not null,   -- 0 none, 1 to_basis, 2 to_parked, 3 to_idle
  state           smallint not null,   -- VaultState at evaluation time
  primary key (vault_symbol, ts)
);

create table public.funding_samples (
  vault_symbol        text not null references public.vaults (symbol),
  ts                  timestamptz not null,
  rate_hourly_scaled  bigint not null,  -- hourly rate in bps x 1e6 (FUNDING_SCALE)
  primary key (vault_symbol, ts)
);

create table public.state_changes (
  id            bigint generated always as identity primary key,
  vault_symbol  text not null references public.vaults (symbol),
  ts            timestamptz not null,
  slot          bigint not null,
  signature     text not null,
  log_index       integer not null,
  from_state    smallint not null,
  to_state      smallint not null,
  step          smallint not null,
  unique (signature, log_index)
);
create index state_changes_vault_ts_idx on public.state_changes (vault_symbol, ts desc);

create table public.rebalances (
  id            bigint generated always as identity primary key,
  vault_symbol  text not null references public.vaults (symbol),
  ts            timestamptz not null,
  slot          bigint not null,
  signature     text not null,
  log_index       integer not null,
  kind          text not null,         -- to_kamino | to_phoenix | from_parked | size_up | unwind_partial
  amount        bigint not null,
  unique (signature, log_index)
);
create index rebalances_vault_ts_idx on public.rebalances (vault_symbol, ts desc);

create table public.epochs (
  vault_symbol          text not null references public.vaults (symbol),
  epoch_id              bigint not null,
  closed_at             timestamptz,
  settled_at            timestamptz,
  shares_total          bigint not null default 0,
  stock_owed            bigint not null default 0,   -- snapshot at close
  usdc_owed             bigint not null default 0,
  stock_paid            bigint not null default 0,   -- actual at settle
  usdc_paid             bigint not null default 0,
  stock_per_share_e6    bigint not null default 0,   -- derived: paid * 1e6 / shares_total
  usdc_per_share_e6     bigint not null default 0,
  primary key (vault_symbol, epoch_id)
);

-- Exits are keyed on (vault, user, nonce) since the second migration; request_signature is kept for the activity feed.
create table public.exits (
  vault_symbol        text not null references public.vaults (symbol),
  user_pubkey         text not null,
  request_signature   text not null,
  nonce               bigint,
  shares              bigint not null,
  epoch_id            bigint not null,
  status              smallint not null,     -- 0 open, 1 settled, 2 redeemed, 3 cancelled
  requested_at        timestamptz not null,
  cancelled_at        timestamptz,
  redeemed_at         timestamptz,
  stock_out           bigint,
  usdc_out            bigint,
  primary key (vault_symbol, user_pubkey, request_signature)
);
create index exits_user_idx on public.exits (user_pubkey);
create index exits_open_idx on public.exits (vault_symbol, user_pubkey, requested_at) where status in (0, 1);
create index exits_redeemed_at_idx on public.exits (redeemed_at desc) where redeemed_at is not null;

create table public.deposits (
  id            bigint generated always as identity primary key,
  vault_symbol  text not null references public.vaults (symbol),
  user_pubkey   text not null,
  ts            timestamptz not null,
  slot          bigint not null,
  signature     text not null,
  log_index       integer not null,
  qty           bigint not null,
  shares        bigint not null,
  unique (signature, log_index)
);
create index deposits_vault_ts_idx on public.deposits (vault_symbol, ts desc);
create index deposits_user_idx on public.deposits (user_pubkey);

create table public.fees (
  id              bigint generated always as identity primary key,
  vault_symbol    text not null references public.vaults (symbol),
  ts              timestamptz not null,
  slot            bigint not null,
  signature       text not null,
  log_index       integer not null,
  shares_minted   bigint not null,
  high_water_e6   bigint not null,
  unique (signature, log_index)
);
create index fees_vault_ts_idx on public.fees (vault_symbol, ts desc);

-- ---------------------------------------------------------------------------
-- Keeper operations (written from the keeper status endpoint; not public)
-- ---------------------------------------------------------------------------
create table public.keeper_snapshots (
  vault_symbol           text not null references public.vaults (symbol),
  ts                     timestamptz not null,
  state                  text not null,
  step                   smallint not null,
  ltv_bps                integer not null,
  margin_bps             integer,
  net_delta_qty          bigint not null,
  carry_accrued_usdc_e6  bigint not null,
  carry_ann_net_bps      bigint not null,
  legs                   jsonb not null,
  primary key (vault_symbol, ts)
);

create table public.keeper_alerts (
  id            bigint generated always as identity primary key,
  ts            timestamptz not null,
  level         text not null check (level in ('warn', 'crit')),
  vault_symbol  text references public.vaults (symbol),
  message       text not null,
  resolved_at   timestamptz
);
create index keeper_alerts_open_idx on public.keeper_alerts (ts desc) where resolved_at is null;
create index keeper_alerts_vault_idx on public.keeper_alerts (vault_symbol);

create table public.keeper_heartbeats (
  instance_id     text primary key,
  is_leader       boolean not null,
  last_hourly_ts  timestamptz,
  last_fast_ts    timestamptz,
  sol_balance     numeric(20, 9) not null default 0,
  updated_at      timestamptz not null default now()
);

-- ---------------------------------------------------------------------------
-- Views (security_invoker so RLS of the caller applies)
-- ---------------------------------------------------------------------------
create view public.v_trailing_yield with (security_invoker = true) as
with latest as (
  select distinct on (vault_symbol) vault_symbol, ts, share_price_stock_e6
  from public.nav_samples
  order by vault_symbol, ts desc
),
first_sample as (
  select distinct on (vault_symbol) vault_symbol, ts, share_price_stock_e6
  from public.nav_samples
  order by vault_symbol, ts asc
)
select
  l.vault_symbol,
  l.ts as as_of,
  l.share_price_stock_e6,
  f.ts as inception_at,
  d7.share_price_stock_e6  as share_price_7d_e6,
  d30.share_price_stock_e6 as share_price_30d_e6,
  f.share_price_stock_e6   as share_price_inception_e6,
  case when d7.share_price_stock_e6  > 0 then (l.share_price_stock_e6 - d7.share_price_stock_e6)  * 10000 / d7.share_price_stock_e6  end as growth_7d_bps,
  case when d30.share_price_stock_e6 > 0 then (l.share_price_stock_e6 - d30.share_price_stock_e6) * 10000 / d30.share_price_stock_e6 end as growth_30d_bps,
  case when f.share_price_stock_e6   > 0 then (l.share_price_stock_e6 - f.share_price_stock_e6)   * 10000 / f.share_price_stock_e6   end as growth_inception_bps
from latest l
join first_sample f on f.vault_symbol = l.vault_symbol
left join lateral (
  select share_price_stock_e6 from public.nav_samples n
  where n.vault_symbol = l.vault_symbol and n.ts <= l.ts - interval '7 days'
  order by n.ts desc limit 1
) d7 on true
left join lateral (
  select share_price_stock_e6 from public.nav_samples n
  where n.vault_symbol = l.vault_symbol and n.ts <= l.ts - interval '30 days'
  order by n.ts desc limit 1
) d30 on true;

create view public.v_funding_24h with (security_invoker = true) as
select vault_symbol, ts, rate_hourly_scaled
from (
  select vault_symbol, ts, rate_hourly_scaled,
         row_number() over (partition by vault_symbol order by ts desc) as rn
  from public.funding_samples
) s
where rn <= 24;

create view public.v_protocol_stats with (security_invoker = true) as
with latest_nav as (
  select distinct on (vault_symbol) vault_symbol, nav_usd_e6
  from public.nav_samples order by vault_symbol, ts desc
),
latest_rule as (
  select distinct on (vault_symbol) vault_symbol, f_avg_bps, parked_apy_bps, state
  from public.rule_samples order by vault_symbol, ts desc
),
per_vault as (
  select n.vault_symbol, n.nav_usd_e6, r.f_avg_bps, r.parked_apy_bps, r.state
  from latest_nav n left join latest_rule r on r.vault_symbol = n.vault_symbol
)
select
  coalesce(sum(nav_usd_e6), 0)::bigint as tvl_usd_e6,
  case when sum(nav_usd_e6) > 0
       then (sum(nav_usd_e6 * coalesce(f_avg_bps, 0)) / sum(nav_usd_e6))::bigint end as tvl_weighted_f_avg_bps,
  case when sum(nav_usd_e6) > 0
       then (sum(nav_usd_e6 * coalesce(parked_apy_bps, 0)) / sum(nav_usd_e6))::bigint end as tvl_weighted_parked_apy_bps,
  count(*) filter (where state = 3)::integer as vaults_in_basis,
  count(*) filter (where state = 1)::integer as vaults_parked,
  count(*) filter (where state = 0)::integer as vaults_idle,
  count(*)::integer as vaults_total,
  (select coalesce(sum(usdc_out), 0)::bigint from public.exits
     where redeemed_at >= now() - interval '24 hours') as usdc_paid_24h,
  (select count(distinct user_pubkey)::integer from public.deposits) as depositors
from per_vault;

-- ---------------------------------------------------------------------------
-- Row level security and grants. New projects do not expose tables to the
-- Data API automatically, so grants are explicit here.
-- ---------------------------------------------------------------------------
alter table public.vaults            enable row level security;
alter table public.program_events    enable row level security;
alter table public.nav_samples       enable row level security;
alter table public.rule_samples      enable row level security;
alter table public.funding_samples   enable row level security;
alter table public.state_changes     enable row level security;
alter table public.rebalances        enable row level security;
alter table public.epochs            enable row level security;
alter table public.exits             enable row level security;
alter table public.deposits          enable row level security;
alter table public.fees              enable row level security;
alter table public.keeper_snapshots  enable row level security;
alter table public.keeper_alerts     enable row level security;
alter table public.keeper_heartbeats enable row level security;

-- Public read tables
create policy "public read" on public.vaults          for select to anon, authenticated using (true);
create policy "public read" on public.nav_samples     for select to anon, authenticated using (true);
create policy "public read" on public.rule_samples    for select to anon, authenticated using (true);
create policy "public read" on public.funding_samples for select to anon, authenticated using (true);
create policy "public read" on public.state_changes   for select to anon, authenticated using (true);
create policy "public read" on public.rebalances      for select to anon, authenticated using (true);
create policy "public read" on public.epochs          for select to anon, authenticated using (true);
create policy "public read" on public.exits           for select to anon, authenticated using (true);
create policy "public read" on public.deposits        for select to anon, authenticated using (true);
create policy "public read" on public.fees            for select to anon, authenticated using (true);

grant select on
  public.vaults, public.nav_samples, public.rule_samples, public.funding_samples,
  public.state_changes, public.rebalances, public.epochs, public.exits,
  public.deposits, public.fees,
  public.v_trailing_yield, public.v_funding_24h, public.v_protocol_stats
to anon, authenticated;

-- program_events and keeper_* are not reachable by anon/authenticated at all.
-- Projects created before the 2026 grant change still carry default privileges
-- on public, so revoke explicitly instead of relying on their absence.
revoke all on public.program_events, public.keeper_snapshots, public.keeper_alerts, public.keeper_heartbeats
  from anon, authenticated;

-- Indexer and keeper write with service_role (bypasses RLS).
grant all on public.vaults, public.program_events, public.nav_samples, public.rule_samples, public.funding_samples,
  public.state_changes, public.rebalances, public.epochs, public.exits, public.deposits, public.fees,
  public.keeper_snapshots, public.keeper_alerts, public.keeper_heartbeats to service_role;
grant usage, select on all sequences in schema public to service_role;
