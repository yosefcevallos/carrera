-- Last 7 days (168 hourly samples) of Phoenix funding per vault, oldest first.
-- funding_samples holds both on-chain FundingRecorded rows (block-time stamped) and rows
-- backfilled from Phoenix's API (hour-stamped); the latest 168 by ts are returned, so the
-- Phoenix series dominates once the indexer has written it.
create view public.v_funding_7d with (security_invoker = true) as
select vault_symbol, ts, rate_hourly_scaled
from (
  select vault_symbol, ts, rate_hourly_scaled,
         row_number() over (partition by vault_symbol order by ts desc) as rn
  from public.funding_samples
) s
where rn <= 168
order by vault_symbol, ts;

grant select on public.v_funding_7d to anon, authenticated;
