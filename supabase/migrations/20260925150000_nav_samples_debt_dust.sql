-- NavRefreshed now carries the tolerated residual debt (DEBT_DUST_USDC write-off), in USDC base units.
alter table public.nav_samples add column if not exists debt_dust_usdc bigint;
comment on column public.nav_samples.debt_dust_usdc is 'Residual debt at or below the dust tolerance at refresh time; null for rows indexed before the field existed.';
