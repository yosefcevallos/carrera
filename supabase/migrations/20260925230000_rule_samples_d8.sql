-- D8: the entry side of the rule is the 3-sample funding average and both sides compare against
-- break-even r + L·r. Both are appended to RuleEvaluated; older rows keep null.
alter table public.rule_samples add column if not exists f_3h_bps bigint;
alter table public.rule_samples add column if not exists be_bps bigint;
