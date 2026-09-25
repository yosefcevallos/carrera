#!/usr/bin/env python3
"""Replicates the klend `Reserve` byte layout (v1.25, sequential, no implicit padding) and checks it
against dumped mainnet reserves. Prints the offsets the on-chain reader in venues/kamino.rs uses.
Usage: python3 kamino_layout.py kamino/reserve_usdc.json kamino/reserve_tslax.json
"""
import base64, json, struct, sys

ALPHABET = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
def b58(b):
    n = int.from_bytes(b, "big"); s = b""
    while n:
        n, r = divmod(n, 58); s = ALPHABET[r:r + 1] + s
    return (ALPHABET[:1] * (len(b) - len(b.lstrip(b"\0"))) + s).decode()

SZ = {"u8": 1, "u16": 2, "u32": 4, "u64": 8, "i64": 8, "u128": 16, "Pubkey": 32}
liquidity = [("mint_pubkey", "Pubkey"), ("supply_vault", "Pubkey"), ("fee_vault", "Pubkey"), ("total_available_amount", "u64"),
             ("borrowed_amount_sf", "u128"), ("market_price_sf", "u128"), ("market_price_last_updated_ts", "u64"),
             ("mint_decimals", "u64"), ("deposit_limit_crossed_timestamp", "u64"), ("borrow_limit_crossed_timestamp", "u64"),
             ("cumulative_borrow_rate_bsf", "48"), ("accumulated_protocol_fees_sf", "u128"), ("accumulated_referrer_fees_sf", "u128"),
             ("pending_referrer_fees_sf", "u128"), ("absolute_referral_rate_sf", "u128"), ("token_program", "Pubkey"),
             ("rewards_amount_available", "u64"), ("padding2", "400"), ("padding3", "512")]
collateral = [("mint_pubkey", "Pubkey"), ("mint_total_supply", "u64"), ("supply_vault", "Pubkey"), ("padding1", "512"), ("padding2", "512")]
fees = [("origination_fee_sf", "u64"), ("flash_loan_fee_sf", "u64"), ("padding", "8")]
token_info = [("name", "32"), ("heuristic", "24"), ("max_twap_divergence_bps", "u64"), ("max_age_price_seconds", "u64"),
              ("max_age_twap_seconds", "u64"), ("scope_price_feed", "Pubkey"), ("scope_price_chain", "8"), ("scope_twap_chain", "8"),
              ("sb_price_aggregator", "Pubkey"), ("sb_twap_aggregator", "Pubkey"), ("pyth_price", "Pubkey"), ("block_price_usage", "u8"),
              ("reserved", "7"), ("_padding", "152")]
wcaps = [("config_capacity", "i64"), ("current_total", "i64"), ("last_interval_start_timestamp", "u64"), ("config_interval_length_seconds", "u64")]
config = [("status", "u8"), ("padding_deprecated_asset_tier", "u8"), ("host_fixed_interest_rate_bps", "u16"), ("min_deleveraging_bonus_bps", "u16"),
          ("block_ctoken_usage", "u8"), ("early_repay_remaining_interest_pct", "u8"), ("emergency_mode", "u8"), ("interest_rate_basis", "u8"),
          ("reserved_1", "3"), ("protocol_order_execution_fee_pct", "u8"), ("protocol_take_rate_pct", "u8"), ("protocol_liquidation_fee_pct", "u8"),
          ("loan_to_value_pct", "u8"), ("liquidation_threshold_pct", "u8"), ("min_liquidation_bonus_bps", "u16"), ("max_liquidation_bonus_bps", "u16"),
          ("bad_debt_liquidation_bonus_bps", "u16"), ("deleveraging_margin_call_period_secs", "u64"), ("deleveraging_threshold_decrease_bps_per_day", "u64"),
          ("fees", fees), ("borrow_rate_curve", "88"), ("borrow_factor_pct", "u64"), ("deposit_limit", "u64"), ("borrow_limit", "u64"),
          ("token_info", token_info), ("deposit_withdrawal_cap", wcaps), ("debt_withdrawal_cap", wcaps), ("elevation_groups", "20"),
          ("disable_usage_as_coll_outside_emode", "u8"), ("utilization_limit_block_borrowing_above_pct", "u8"), ("autodeleverage_enabled", "u8"),
          ("proposer_authority_locked", "u8"), ("borrow_limit_outside_elevation_group", "u64"),
          ("borrow_limit_against_this_collateral_in_elevation_group", "256"), ("deleveraging_bonus_increase_bps_per_day", "u64"),
          ("debt_maturity_timestamp", "u64"), ("debt_term_seconds", "u64"), ("rewards_amount_per_accrual_unit", "u64"), ("permissioned_ops", "u64")]
reserve = [("version", "u64"), ("last_update", "16"), ("lending_market", "Pubkey"), ("farm_collateral", "Pubkey"), ("farm_debt", "Pubkey"),
           ("liquidity", liquidity), ("reserve_liquidity_padding", "1200"), ("collateral", collateral), ("reserve_collateral_padding", "1200"),
           ("config", config), ("config_padding", "896"), ("borrowed_amount_outside_elevation_group", "u64"),
           ("borrowed_amounts_against_this_reserve_in_elevation_groups", "256"), ("withdraw_queue", "24"), ("padding", "1632")]

OFFS = {}
def walk(fields, base, prefix):
    o = base
    for n, t in fields:
        if isinstance(t, list):
            o = walk(t, o, prefix + n + ".")
        else:
            OFFS[prefix + n] = o
            o += int(t) if t.isdigit() else SZ[t]
    return o

TOTAL = walk(reserve, 0, "")

def main():
    print("computed Reserve size", TOTAL, "(klend: 8616 + 8 discriminator = 8624 account bytes)")
    keys = ["lending_market", "farm_collateral", "liquidity.mint_pubkey", "liquidity.supply_vault", "liquidity.fee_vault",
            "liquidity.total_available_amount", "liquidity.borrowed_amount_sf", "liquidity.market_price_sf",
            "liquidity.market_price_last_updated_ts", "liquidity.mint_decimals", "liquidity.token_program", "collateral.mint_pubkey",
            "collateral.supply_vault", "config.status", "config.protocol_take_rate_pct", "config.loan_to_value_pct",
            "config.liquidation_threshold_pct", "config.borrow_rate_curve", "config.token_info.scope_price_feed",
            "config.token_info.scope_price_chain", "config.token_info.pyth_price", "config.token_info.sb_price_aggregator"]
    print("offsets (from start of data after the 8-byte discriminator):")
    for k in keys:
        print(f"  {k:48} {OFFS[k]}")
    for path in sys.argv[1:]:
        d = base64.b64decode(json.load(open(path))["data_base64"])
        assert len(d) == TOTAL + 8, f"{path}: {len(d)} bytes, layout says {TOTAL + 8}"
        d = d[8:]
        pk = lambda k: b58(d[OFFS[k]:OFFS[k] + 32])
        u64 = lambda k: struct.unpack_from("<Q", d, OFFS[k])[0]
        u128 = lambda k: int.from_bytes(d[OFFS[k]:OFFS[k] + 16], "little")
        print("==", path)
        print("  liq.mint", pk("liquidity.mint_pubkey"), "decimals", u64("liquidity.mint_decimals"), "token_program", pk("liquidity.token_program"))
        print("  price", u128("liquidity.market_price_sf") / 2 ** 60, "price_ts", u64("liquidity.market_price_last_updated_ts"),
              "available", u64("liquidity.total_available_amount"), "borrowed", u128("liquidity.borrowed_amount_sf") / 2 ** 60)
        print("  supply_vault", pk("liquidity.supply_vault"), "fee_vault", pk("liquidity.fee_vault"))
        print("  coll.mint", pk("collateral.mint_pubkey"), "coll.supply", pk("collateral.supply_vault"))
        print("  farm_collateral", pk("farm_collateral"), "market", pk("lending_market"))
        print("  status", d[OFFS["config.status"]], "take_rate%", d[OFFS["config.protocol_take_rate_pct"]], "ltv%",
              d[OFFS["config.loan_to_value_pct"]], "liq%", d[OFFS["config.liquidation_threshold_pct"]])
        c = OFFS["config.borrow_rate_curve"]
        print("  curve", [struct.unpack_from("<II", d, c + 8 * i) for i in range(11)])
        print("  scope feed", pk("config.token_info.scope_price_feed"), "chain", struct.unpack_from("<4H", d, OFFS["config.token_info.scope_price_chain"]),
              "pyth", pk("config.token_info.pyth_price"), "switchboard", pk("config.token_info.sb_price_aggregator"))

if __name__ == "__main__":
    main()
