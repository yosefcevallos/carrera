-- Exit events now carry the request nonce, so exits key on (vault, user, nonce).
-- request_signature is kept for the activity feed.
alter table public.exits drop constraint exits_pkey;
alter table public.exits alter column nonce set not null;
alter table public.exits add primary key (vault_symbol, user_pubkey, nonce);
drop index if exists public.exits_open_idx;
create index exits_signature_idx on public.exits (request_signature);
