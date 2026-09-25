#!/usr/bin/env python3
"""Dump mainnet accounts (and upgradeable program ELFs) into JSON fixtures for the fork tests.

Usage: python3 dump.py <rpc-url-file-or-url> <out-dir> <name=pubkey> [name=pubkey ...]
       add `program:` prefix to a name to dump an upgradeable program's ELF (from its programdata,
       trimmed to the ELF's real extent so the fixture is not padded to the allocated size).
Fixture shape: {"pubkey", "owner", "lamports", "executable", "data_base64"[, "elf": true]}.
Dump every account a scenario needs in ONE invocation: Kamino checks reserve fields against the
reserve's token vault balances, so reserves and vaults must come from the same moment.
"""
import base64, json, os, sys, urllib.request

def rpc(url, method, params):
    req = urllib.request.Request(url, data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
                                 headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        out = json.load(r)
    if "error" in out:
        raise SystemExit(f"{method} {params[0]}: {out['error']}")
    return out["result"]

def account(url, pk):
    v = rpc(url, "getAccountInfo", [pk, {"encoding": "base64", "commitment": "confirmed"}])["value"]
    if v is None:
        raise SystemExit(f"{pk}: not found")
    return {"pubkey": pk, "owner": v["owner"], "lamports": v["lamports"], "executable": v["executable"],
            "data_base64": v["data"][0]}

def trim_elf(b: bytes) -> bytes:
    assert b[:4] == b"\x7fELF", "not an ELF"
    shoff = int.from_bytes(b[0x28:0x30], "little")
    shentsize = int.from_bytes(b[0x3a:0x3c], "little")
    shnum = int.from_bytes(b[0x3c:0x3e], "little")
    return b[: min(len(b), shoff + shentsize * shnum)]

def main():
    src, out_dir, *pairs = sys.argv[1:]
    url = open(src).read().strip() if os.path.exists(src) else src
    os.makedirs(out_dir, exist_ok=True)
    for pair in pairs:
        name, pk = pair.split("=", 1)
        if name.startswith("program:"):
            name = name[len("program:"):]
            prog = account(url, pk)
            raw = base64.b64decode(prog["data_base64"])
            assert raw[:4] == b"\x02\x00\x00\x00", "not an upgradeable program account"
            pd = rpc(url, "getAccountInfo", [b58(raw[4:36]), {"encoding": "base64", "commitment": "confirmed"}])["value"]
            elf = trim_elf(base64.b64decode(pd["data"][0])[45:])  # UpgradeableLoaderState::ProgramData header
            fixture = {"pubkey": pk, "owner": prog["owner"], "lamports": prog["lamports"], "executable": True,
                       "data_base64": base64.b64encode(elf).decode(), "elf": True}
        else:
            fixture = account(url, pk)
        path = os.path.join(out_dir, f"{name}.json")
        json.dump(fixture, open(path, "w"))
        print(f"{name:28} {pk} owner={fixture['owner'][:8]} bytes={len(base64.b64decode(fixture['data_base64']))}")

ALPHABET = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
def b58(b: bytes) -> str:
    n = int.from_bytes(b, "big"); s = b""
    while n:
        n, r = divmod(n, 58); s = ALPHABET[r:r + 1] + s
    pad = len(b) - len(b.lstrip(b"\0"))
    return (ALPHABET[:1] * pad + s).decode()

if __name__ == "__main__":
    main()
