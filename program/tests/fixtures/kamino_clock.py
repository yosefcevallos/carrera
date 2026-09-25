#!/usr/bin/env python3
"""Print the slots/timestamps the fork test must run at: reserve last_update.slot and the Scope
DatedPrice entries for the USDC (13) and TSLAx (338) chains. Usage: python3 kamino_clock.py"""
import base64, json, os, struct

here = os.path.dirname(os.path.abspath(__file__))
def data(name):
    return base64.b64decode(json.load(open(os.path.join(here, "kamino", name + ".json")))["data_base64"])

for r in ["reserve_usdc", "reserve_tslax"]:
    d = data(r)[8:]
    slot = struct.unpack_from("<Q", d, 8)[0]
    stale = d[16]
    price_ts = struct.unpack_from("<Q", d, 256)[0]
    max_age = struct.unpack_from("<Q", d, 4912 + 88 + 24 + 32 + 24 + 8)[0]
    print(f"{r:14} last_update.slot={slot} stale={stale} price_ts={price_ts} max_age_price_seconds={max_age}")

s = data("scope_prices")
# OraclePrices: disc 8, oracle_mappings 32, prices [DatedPrice; 512] with DatedPrice = 56 bytes:
# price {value u64, exp u64}, last_updated_slot u64, unix_timestamp u64, 24 bytes reserved/index
for idx in [13, 338]:
    o = 8 + 32 + idx * 56
    value, exp, slot, ts = struct.unpack_from("<QQQQ", s, o)
    print(f"scope[{idx:3}] price={value / 10 ** exp:.4f} slot={slot} ts={ts}")
