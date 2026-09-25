//! Mainnet-fork test of the Kamino legs against the plain (non-mock) build.
//!
//! Loads `carrera_overlay` (plain), klend and Farms with the accounts dumped in
//! `../tests/fixtures/kamino/` (Kamino xStocks market, TSLAx + USDC reserves and
//! their vaults, Scope prices, the mints) and drives:
//!
//! deposit (custody) → init_kamino_obligation → sync_collateral (Kamino deposit)
//! → record_kamino_rates (on-chain) → refresh_nav (refresh_reserve + on-chain price)
//! → 24 × record_funding → wind_start from Idle (Kamino borrow) → wind_abort →
//! unwind 1..3 + commit (Kamino supply) → repay by guardian (Kamino redeem + repay)
//! → request_exit → close_epoch → settle_epoch (Kamino withdraw) → redeem.
//!
//! Jupiter and Phoenix are not exercised: routes are slot-bound and Phoenix needs
//! its live trader-index buffers.

use litesvm::LiteSVM;
use sha2::{Digest, Sha256};
use solana_account::Account;
use solana_address::Address;
use solana_clock::Clock;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::path::PathBuf;

const PROGRAM_ID: &str = "GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw";
const KLEND: &str = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD";
const FARMS: &str = "FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr";
const MARKET: &str = "5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua";
const RESERVE_TSLA: &str = "5iTiczqgUegqA3PpoNpotizMbY9n1sRWr3oL6igKvWuf";
const RESERVE_USDC: &str = "97zoywd8mPZsGTg8q1wdD2Wgkdrs2tqusp1Qqcxbyj7E";
const SCOPE: &str = "3t4JZcueEzTbVP6kLxXrL3VpWx45jDer4eqysweBchNH";
const TSLA_MINT: &str = "XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const TSLA_LIQ_SUPPLY: &str = "AvhRUjab47DCo9efnzmDha8xUeQFEs36Yywv1x8t3T2W";
const TSLA_COLL_MINT: &str = "6bZpUNY1qmbvQBgCmfQJUA377X63ATvnpCHYh8hQnfjC";
const TSLA_COLL_SUPPLY: &str = "5nFXkYBqkEmKZHFabSiiyjX2XVxUT9jfuUkLFoVTvVk5";
const USDC_LIQ_SUPPLY: &str = "72Gz8BM8vDr5zdeHmFB5A2ptNZYfVgDuw5LDkNwGDdTA";
const USDC_FEE_VAULT: &str = "DPHXY8LbH4cPPAgBDNnrU7B2XwmhWcBZ9wTBi3uZpfh4";
const USDC_COLL_MINT: &str = "69nwLK2t639e2c2ZWmZAbs2HWKZwG4pXVPzBTiEtNmwf";
const USDC_DEBT_FARM: &str = "82eHAjSXZEyA3UpBxTjVYXF4QJmAEtLR6kvWXQca7mqd";
const TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const ATA_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const SYSTEM: &str = "11111111111111111111111111111111";
const RENT: &str = "SysvarRent111111111111111111111111111111111";
const IX_SYSVAR: &str = "Sysvar1nstructions1111111111111111111111111";

/// Newest Scope DatedPrice in the fixture (index 338, TSLAx): the fork clock starts just after it,
/// and every warp re-stamps the two Scope entries so Kamino's price-age checks (180 s USDC, 300 s
/// TSLAx) keep passing. See tests/fixtures/kamino_clock.py.
const DUMP_TS: i64 = 1_790_300_787;
const DUMP_SLOT: u64 = 450_208_310;
/// Scope chain entries the two reserves read: USDC price 13 / TWAP 456, TSLAx price 338 / TWAP 273
/// (reserve.config.token_info.scope_configuration.{price_chain, twap_chain}).
const SCOPE_INDICES: [usize; 4] = [13, 456, 338, 273];

fn a(s: &str) -> Address {
    s.parse().unwrap()
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")
}

struct Fixture {
    pubkey: Address,
    account: Account,
    elf: bool,
}

fn load_fixture(name: &str) -> Fixture {
    let path = fixtures_dir().join("kamino").join(format!("{name}.json"));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).expect(name)).unwrap();
    let data = b64(v["data_base64"].as_str().unwrap());
    Fixture {
        pubkey: a(v["pubkey"].as_str().unwrap()),
        account: Account {
            lamports: v["lamports"].as_u64().unwrap(),
            data,
            owner: a(v["owner"].as_str().unwrap()),
            executable: v["executable"].as_bool().unwrap_or(false),
            rent_epoch: 0,
        },
        elf: v["elf"].as_bool().unwrap_or(false),
    }
}

/// Programdata dumps are padded to the allocated size; trim to the ELF's real extent.
fn trim_elf(bytes: &[u8]) -> &[u8] {
    assert_eq!(&bytes[..4], b"\x7fELF");
    let shoff = u64::from_le_bytes(bytes[0x28..0x30].try_into().unwrap()) as usize;
    let shentsize = u16::from_le_bytes(bytes[0x3a..0x3c].try_into().unwrap()) as usize;
    let shnum = u16::from_le_bytes(bytes[0x3c..0x3e].try_into().unwrap()) as usize;
    let end = shoff + shentsize * shnum;
    &bytes[..end.min(bytes.len())]
}

fn b64(s: &str) -> Vec<u8> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0);
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let v = T.iter().position(|&t| t == c).unwrap() as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

fn disc(name: &str) -> Vec<u8> {
    Sha256::digest(format!("global:{name}").as_bytes())[..8].to_vec()
}

fn data(name: &str, args: &[u8]) -> Vec<u8> {
    let mut d = disc(name);
    d.extend_from_slice(args);
    d
}

fn pda(seeds: &[&[u8]], program: &Address) -> Address {
    Address::find_program_address(seeds, program).0
}

fn w(k: Address) -> AccountMeta {
    AccountMeta::new(k, false)
}
fn r(k: Address) -> AccountMeta {
    AccountMeta::new_readonly(k, false)
}
fn ws(k: Address) -> AccountMeta {
    AccountMeta::new(k, true)
}
fn rs(k: Address) -> AccountMeta {
    AccountMeta::new_readonly(k, true)
}

/// VaultParams in Borsh field order (tier B: TSLA).
fn vault_params() -> Vec<u8> {
    let mut b = Vec::new();
    for v in [3000u32, 1200, 6500, 6000, 200, 100, 50, 450, 720, 60] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.push(24); // funding_window
    for v in [50u32, 30, 50, 50, 300, 800, 500] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&9000u64.to_le_bytes()); // max_nav_age_slots
    b.extend_from_slice(&3600u64.to_le_bytes()); // epoch_len_secs
    b.extend_from_slice(&1500u32.to_le_bytes());
    b.extend_from_slice(&10u32.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes()); // deposit cap: none
    b.extend_from_slice(&0u64.to_le_bytes()); // basis cap: none
    b
}

/// VenueData { blocks: KAMINO, zeros..., jupiter_data: [] }
fn venue_kamino() -> Vec<u8> {
    let mut b = vec![1u8, 0, 0];
    b.extend_from_slice(&[0u8; 8 * 5]);
    b.extend_from_slice(&0u32.to_le_bytes());
    b
}
fn vec_arg(v: &[u8]) -> Vec<u8> {
    let mut b = (v.len() as u32).to_le_bytes().to_vec();
    b.extend_from_slice(v);
    b
}

struct World {
    svm: LiteSVM,
    program: Address,
    admin: Keypair,
    keeper: Keypair,
    user: Keypair,
    registry: Address,
    vault: Address,
    share_mint: Address,
    stock_custody: Address,
    usdc_buffer: Address,
    redeem_stock: Address,
    redeem_usdc: Address,
    obligation: Address,
    user_meta: Address,
    vault_usdc_ctoken: Address,
    user_stock: Address,
    user_shares: Address,
    user_usdc: Address,
    clock_ts: i64,
    clock_slot: u64,
}

impl World {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let program = a(PROGRAM_ID);
        let plain = std::fs::read(fixtures_dir().join("carrera_overlay.plain.so")).expect("plain build; run `anchor build`");
        svm.add_program(program, &plain).unwrap();
        for prog in ["klend", "farms"] {
            let f = load_fixture(prog);
            assert!(f.elf);
            svm.add_program(f.pubkey, trim_elf(&f.account.data)).unwrap();
        }
        for name in [
            "lending_market", "reserve_usdc", "reserve_tslax", "scope_prices", "tslax_liq_supply", "tslax_coll_mint",
            "tslax_coll_supply", "usdc_liq_supply", "usdc_fee_vault", "usdc_coll_mint", "usdc_coll_supply", "tslax_mint",
            "usdc_mint", "usdc_debt_farm", "farms_global_config",
        ] {
            let f = load_fixture(name);
            svm.set_account(f.pubkey, f.account).unwrap();
        }
        let admin = Keypair::new();
        let keeper = Keypair::new();
        let user = Keypair::new();
        for k in [&admin, &keeper, &user] {
            svm.airdrop(&k.pubkey(), 50_000_000_000).unwrap();
        }
        let registry = pda(&[b"registry"], &program);
        let mint = a(TSLA_MINT);
        let vault = pda(&[b"vault", mint.as_ref()], &program);
        let share_mint = pda(&[b"shares", vault.as_ref()], &program);
        let sys = a(SYSTEM);
        let market = a(MARKET);
        let mut w = Self {
            stock_custody: pda(&[b"stock", vault.as_ref()], &program),
            usdc_buffer: pda(&[b"usdc", vault.as_ref()], &program),
            redeem_stock: pda(&[b"redeem_stock", vault.as_ref()], &program),
            redeem_usdc: pda(&[b"redeem_usdc", vault.as_ref()], &program),
            obligation: pda(&[&[0u8], &[0u8], vault.as_ref(), market.as_ref(), sys.as_ref(), sys.as_ref()], &a(KLEND)),
            user_meta: pda(&[b"user_meta", vault.as_ref()], &a(KLEND)),
            vault_usdc_ctoken: ata(&vault, &a(USDC_COLL_MINT), &a(TOKEN)),
            user_stock: ata(&user.pubkey(), &mint, &a(TOKEN_2022)),
            user_shares: ata(&user.pubkey(), &share_mint, &a(TOKEN)),
            user_usdc: ata(&user.pubkey(), &a(USDC_MINT), &a(TOKEN)),
            svm, program, admin, keeper, user, registry, vault, share_mint,
            clock_ts: DUMP_TS + 5,
            clock_slot: DUMP_SLOT,
        };
        w.set_clock(0, 0);
        w
    }

    fn set_clock(&mut self, add_secs: i64, add_slots: u64) {
        self.clock_ts += add_secs;
        self.clock_slot += add_slots;
        let clock = Clock {
            slot: self.clock_slot,
            epoch_start_timestamp: self.clock_ts - 1000,
            epoch: 1,
            leader_schedule_epoch: 1,
            unix_timestamp: self.clock_ts,
        };
        self.svm.set_sysvar(&clock);
        self.svm.expire_blockhash();
        // Re-stamp the Scope prices the two reserves read (DatedPrice: price 16, slot 8, ts 8, 24 reserved).
        let scope = a(SCOPE);
        let mut acc = self.svm.get_account(&scope).unwrap();
        for idx in SCOPE_INDICES {
            let o = 8 + 32 + idx * 56;
            acc.data[o + 16..o + 24].copy_from_slice(&self.clock_slot.to_le_bytes());
            acc.data[o + 24..o + 32].copy_from_slice(&(self.clock_ts as u64).to_le_bytes());
        }
        self.svm.set_account(scope, acc).unwrap();
    }

    fn send(&mut self, label: &str, ixs: Vec<Instruction>, signers: &[&Keypair]) -> Vec<String> {
        let mut all = vec![ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)];
        all.extend(ixs);
        let payer = signers[0].pubkey();
        let msg = Message::new(&all, Some(&payer));
        let tx = Transaction::new(signers, msg, self.svm.latest_blockhash());
        match self.svm.send_transaction(tx) {
            Ok(meta) => meta.logs,
            Err(e) => {
                eprintln!("--- {label} failed: {:?}", e.err);
                for l in &e.meta.logs {
                    eprintln!("    {l}");
                }
                panic!("{label} failed");
            }
        }
    }

    fn token_amount(&self, k: &Address) -> u64 {
        let acc = self.svm.get_account(k).unwrap_or_default();
        if acc.data.len() < 72 {
            return 0;
        }
        u64::from_le_bytes(acc.data[64..72].try_into().unwrap())
    }

    /// Kamino block per docs/CONTRACT.md "Venue blocks".
    fn kamino_block(&self) -> Vec<AccountMeta> {
        let market = a(MARKET);
        vec![
            r(a(KLEND)),
            r(market),
            r(pda(&[b"lma", market.as_ref()], &a(KLEND))),
            w(self.obligation),
            w(a(RESERVE_TSLA)),
            w(a(TSLA_LIQ_SUPPLY)),
            w(a(TSLA_COLL_MINT)),
            w(a(TSLA_COLL_SUPPLY)),
            r(a(TSLA_MINT)),
            w(a(RESERVE_USDC)),
            w(a(USDC_LIQ_SUPPLY)),
            w(a(USDC_FEE_VAULT)),
            w(a(USDC_COLL_MINT)),
            w(self.vault_usdc_ctoken),
            r(a(USDC_MINT)),
            r(a(SCOPE)),
            r(a(FARMS)),
            r(a(IX_SYSVAR)),
            r(a(TOKEN)),
            r(a(TOKEN_2022)),
            w(self.stock_custody),
            w(self.usdc_buffer),
            w(a(USDC_DEBT_FARM)),
            w(pda(&[b"user", a(USDC_DEBT_FARM).as_ref(), self.obligation.as_ref()], &a(FARMS))),
        ]
    }

    fn keeper_vault(&self, name: &str, args: &[u8], block: bool) -> Instruction {
        let mut metas = vec![ws(self.keeper.pubkey()), w(self.registry), w(self.vault)];
        let mut d = args.to_vec();
        if block {
            metas.extend(self.kamino_block());
            d.extend(vec_arg(&venue_kamino()));
        } else {
            d.extend(vec_arg(&[]));
        }
        Instruction { program_id: self.program, accounts: metas, data: data(name, &d) }
    }

    fn vault_field_u64(&self, offset: usize) -> u64 {
        let acc = self.svm.get_account(&self.vault).unwrap();
        u64::from_le_bytes(acc.data[offset..offset + 8].try_into().unwrap())
    }
    fn vault_state(&self) -> u8 {
        // discriminator 8 + xstock_mint 32 + share_mint 32 + tier 1 + VaultParams
        let acc = self.svm.get_account(&self.vault).unwrap();
        acc.data[8 + 32 + 32 + 1 + vault_params().len()]
    }
}

fn ata(owner: &Address, mint: &Address, token_program: &Address) -> Address {
    pda(&[owner.as_ref(), token_program.as_ref(), mint.as_ref()], &a(ATA_PROGRAM))
}

fn create_ata_ix(payer: &Address, owner: &Address, mint: &Address, token_program: &Address) -> Instruction {
    Instruction {
        program_id: a(ATA_PROGRAM),
        accounts: vec![ws(*payer), w(ata(owner, mint, token_program)), r(*owner), r(*mint), r(a(SYSTEM)), r(*token_program)],
        data: vec![1], // CreateIdempotent
    }
}

/// Vault account layout offsets after the discriminator (see program state.rs).
mod off {
    pub const HEADER: usize = 8 + 32 + 32 + 1; // disc, xstock_mint, share_mint, tier
    pub fn after_params(params_len: usize) -> usize {
        HEADER + params_len
    }
}

#[test]
fn kamino_legs_end_to_end() {
    let mut t = World::new();
    let params = vault_params();
    let plen = params.len();
    // Offsets into the vault account: state, step, market_open, collateral_qty, basis_spot_qty, debt_usdc, debt_b_usdc, parked_usdc
    let o_collateral = off::after_params(plen) + 3;
    let o_debt = o_collateral + 16;
    let o_parked = o_debt + 16;

    // ---- init registry + vault
    let mut init_reg = vec![1u8]; // hmm: guardian: Pubkey then keepers: Vec<Pubkey>
    init_reg.clear();
    init_reg.extend_from_slice(t.admin.pubkey().as_ref()); // guardian = admin
    init_reg.extend_from_slice(&1u32.to_le_bytes());
    init_reg.extend_from_slice(t.keeper.pubkey().as_ref());
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![ws(t.admin.pubkey()), w(t.registry), r(a(USDC_MINT)), r(a(SYSTEM))],
        data: data("init_registry", &init_reg),
    };
    let admin = t.admin.insecure_clone();
    t.send("init_registry", vec![ix], &[&admin]);

    let mut init_vault = vec![1u8]; // tier B
    init_vault.extend_from_slice(&params);
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![
            ws(t.admin.pubkey()), r(t.registry), w(t.vault), r(a(TSLA_MINT)), w(t.share_mint), w(t.stock_custody),
            w(t.usdc_buffer), w(t.redeem_stock), w(t.redeem_usdc), r(a(USDC_MINT)), r(a(TOKEN)), r(a(SYSTEM)), r(a(RENT)),
            r(a(TOKEN_2022)),
        ],
        data: data("init_vault", &init_vault),
    };
    t.send("init_vault", vec![ix], &[&admin]);

    // ---- user gets 1 TSLAx: clone the reserve's Token-2022 account layout (same extensions), rewrite owner + amount
    let template = load_fixture("tslax_liq_supply").account;
    let mut acc = template.clone();
    acc.data[32..64].copy_from_slice(t.user.pubkey().as_ref());
    acc.data[64..72].copy_from_slice(&100_000_000u64.to_le_bytes());
    t.svm.set_account(t.user_stock, acc).unwrap();
    assert_eq!(t.token_amount(&t.user_stock), 100_000_000);

    // ---- ATAs: user shares, user usdc, vault cToken account
    let user = t.user.insecure_clone();
    let keeper = t.keeper.insecure_clone();
    t.send(
        "atas",
        vec![
            create_ata_ix(&user.pubkey(), &user.pubkey(), &t.share_mint, &a(TOKEN)),
            create_ata_ix(&user.pubkey(), &user.pubkey(), &a(USDC_MINT), &a(TOKEN)),
            create_ata_ix(&user.pubkey(), &t.vault, &a(USDC_COLL_MINT), &a(TOKEN)),
        ],
        &[&user],
    );

    // ---- on-chain Kamino reads: rates from the USDC reserve, price from the TSLAx reserve (with refresh_reserve)
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![rs(keeper.pubkey()), w(t.registry), r(a(RESERVE_USDC))],
        data: data("record_kamino_rates", &[0, 0]), // None, None
    };
    t.send("record_kamino_rates", vec![ix], &[&keeper]);
    let reg = t.svm.get_account(&t.registry).unwrap();
    // Registry: disc 8, admin 32, guardian 32, keepers 4×32, keeper_count 1, paused 1, usdc_mint 32 → borrow_apy at 8+32+32+128+1+1+32
    let o = 8 + 32 + 32 + 128 + 1 + 1 + 32;
    let borrow = u32::from_le_bytes(reg.data[o..o + 4].try_into().unwrap());
    let supply = u32::from_le_bytes(reg.data[o + 4..o + 8].try_into().unwrap());
    eprintln!("on-chain rates: borrow {borrow} bps, supply {supply} bps");
    assert!((300..1500).contains(&borrow) && supply < borrow);

    let refresh = |t: &World| Instruction {
        program_id: t.program,
        accounts: vec![rs(t.keeper.pubkey()), r(t.registry), w(t.vault), w(a(RESERVE_TSLA)), r(a(KLEND)), r(a(MARKET)), r(a(SCOPE))],
        data: data("refresh_nav", &[0]), // None
    };
    let ix = refresh(&t);
    t.send("refresh_nav", vec![ix], &[&keeper]);
    // parked(8), phoenix_equity(8), phoenix_short(8), funding[24](192), head(1), samples(1), last_ts(8), nav(8), share_price(8) → price_e6
    let o_price = o_parked + 8 + 8 + 8 + 8 * 24 + 1 + 1 + 8 + 8 + 8;
    let price = t.vault_field_u64(o_price);
    eprintln!("on-chain TSLAx price: {} USD", price as f64 / 1e6);
    assert!((100_000_000..1_000_000_000).contains(&price));

    // ---- deposit 1 TSLAx into custody
    let mut dep = 100_000_000u64.to_le_bytes().to_vec();
    dep.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![
            ws(user.pubkey()), r(t.registry), w(t.vault), w(t.share_mint), r(a(TSLA_MINT)), w(t.stock_custody), w(t.user_stock),
            w(t.user_shares), r(a(TOKEN)), r(a(TOKEN_2022)),
        ],
        data: data("deposit", &dep),
    };
    t.send("deposit", vec![ix], &[&user]);
    assert_eq!(t.token_amount(&t.stock_custody), 100_000_000);
    assert_eq!(t.vault_field_u64(o_collateral), 100_000_000);

    // ---- init the Kamino obligation (user metadata + obligation) via CPI
    let mut metas = vec![ws(keeper.pubkey()), r(t.registry), w(t.vault), r(a(SYSTEM)), r(a(RENT))];
    metas.extend(t.kamino_block());
    metas.push(w(t.user_meta));
    let ix = Instruction { program_id: t.program, accounts: metas, data: data("init_kamino_obligation", &vec_arg(&venue_kamino())) };
    t.send("init_kamino_obligation", vec![ix], &[&keeper]);
    assert!(t.svm.get_account(&t.obligation).map(|x| x.data.len() > 8).unwrap_or(false), "obligation created");

    // ---- sync custody into Kamino as collateral
    let ix = t.keeper_vault("sync_collateral", &[], true);
    t.send("sync_collateral", vec![ix], &[&keeper]);
    assert_eq!(t.token_amount(&t.stock_custody), 0, "custody moved into the obligation");
    {
        // Where does klend store the deposit reserve key? (documents the obligation layout the program scans)
        let ob = t.svm.get_account(&t.obligation).unwrap().data;
        let key = a(RESERVE_TSLA);
        let hits: Vec<usize> = (0..ob.len() - 32).filter(|&o| &ob[o..o + 32] == key.as_ref()).collect();
        eprintln!("obligation: {} bytes, stock reserve key at offsets {:?} (expected 8 + 88 = 96)", ob.len(), hits);
        assert!(hits.contains(&96), "deposit reserve not at the expected offset");
    }

    // ---- 24 funding samples (non-mock spacing is 59 min) and market open
    for i in 0..24 {
        t.set_clock(3600, 9000);
        let ix = Instruction {
            program_id: t.program,
            accounts: vec![rs(keeper.pubkey()), r(t.registry), w(t.vault), r(a(SYSTEM))],
            data: data("record_funding", &{
                let mut d = vec![1u8];
                d.extend_from_slice(&5_000_000i64.to_le_bytes()); // 5 bps/h scaled ×1e6 → 438%/yr, clears any hurdle
                d
            }),
        };
        t.send(&format!("record_funding {i}"), vec![ix], &[&keeper]);
    }
    let ix = t.keeper_vault("set_market_open", &[1], false);
    // set_market_open has no venue_data arg: rebuild without it
    let ix = Instruction { program_id: ix.program_id, accounts: ix.accounts, data: data("set_market_open", &[1]) };
    t.send("set_market_open", vec![ix], &[&keeper]);
    let ix = refresh(&t);
    t.send("refresh_nav (fresh)", vec![ix], &[&keeper]);

    // ---- wind_start from Idle: Kamino borrow of D = 30% × collateral value into usdc_buffer
    let ix = t.keeper_vault("wind_start", &[], true);
    t.send("wind_start", vec![ix], &[&keeper]);
    let debt = t.vault_field_u64(o_debt);
    let buffer = t.token_amount(&t.usdc_buffer);
    eprintln!("borrowed D = {} USDC (buffer holds {} after Kamino's origination fee)", debt as f64 / 1e6, buffer as f64 / 1e6);
    assert!(debt > 0 && buffer > 0 && buffer <= debt);
    assert_eq!(t.vault_state(), 2, "Winding");

    // ---- abort the wind (no Jupiter in the fork) and unwind back to Parked: supply the USDC on Kamino
    let ix = Instruction { program_id: t.program, accounts: vec![ws(keeper.pubkey()), w(t.registry), w(t.vault)], data: data("wind_abort", &[]) };
    t.send("wind_abort", vec![ix], &[&keeper]);
    for n in 1..=3u8 {
        let ix = t.keeper_vault("unwind_step", &[n], true);
        t.send(&format!("unwind_step {n}"), vec![ix], &[&keeper]);
    }
    let ix = t.keeper_vault("unwind_commit", &[], true);
    t.send("unwind_commit", vec![ix], &[&keeper]);
    assert_eq!(t.vault_state(), 1, "Parked");
    let ctokens = t.token_amount(&t.vault_usdc_ctoken);
    eprintln!("supplied on Kamino: {} cTokens", ctokens);
    assert!(ctokens > 0);

    // ---- keeper cushion: 0.05 USDC into the buffer for accrued interest and cToken rounding
    {
        let mut acc = t.svm.get_account(&t.usdc_buffer).unwrap();
        let cur = u64::from_le_bytes(acc.data[64..72].try_into().unwrap());
        acc.data[64..72].copy_from_slice(&(cur + 50_000).to_le_bytes());
        t.svm.set_account(t.usdc_buffer, acc).unwrap();
    }
    // ---- guardian repay: redeem the supply and repay the whole loan (Kamino settles accrued interest)
    let mut metas = vec![ws(admin.pubkey()), w(t.registry), w(t.vault)];
    metas.extend(t.kamino_block());
    let ix = Instruction { program_id: t.program, accounts: metas, data: data("repay", &vec_arg(&venue_kamino())) };
    t.send("repay (guardian)", vec![ix], &[&admin]);
    assert_eq!(t.vault_state(), 0, "Idle");
    let debt_after = t.vault_field_u64(o_debt);
    eprintln!("debt after repay: {} (origination fee shortfall below dust is written off)", debt_after);
    assert_eq!(debt_after, 0, "loan fully repaid through Kamino's repay-all");
    let left = t.token_amount(&t.usdc_buffer);
    eprintln!("buffer after repay-all: {} USDC (cushion minus accrued interest)", left as f64 / 1e6);
    assert!(left < 50_000, "buffer should have paid interest out of the cushion");

    // ---- exit the whole position: request → close → settle (Kamino withdraw) → redeem
    let mut req = 100_000_000u64.to_le_bytes().to_vec();
    req.extend_from_slice(&7u64.to_le_bytes());
    let exit_request = pda(&[b"exit", t.vault.as_ref(), user.pubkey().as_ref(), &7u64.to_le_bytes()], &t.program);
    let epoch_id = 0u64;
    let exit_epoch = pda(&[b"epoch", t.vault.as_ref(), &epoch_id.to_le_bytes()], &t.program);
    let escrow = pda(&[b"escrow", t.vault.as_ref()], &t.program);
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![
            ws(user.pubkey()), r(t.registry), w(t.vault), w(exit_request), w(exit_epoch), r(t.share_mint), w(t.user_shares),
            w(escrow), r(a(TOKEN)), r(a(SYSTEM)),
        ],
        data: data("request_exit", &req),
    };
    t.send("request_exit", vec![ix], &[&user]);

    t.set_clock(3601, 9000);
    let ix = refresh(&t);
    t.send("refresh_nav (epoch)", vec![ix], &[&keeper]);
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![ws(keeper.pubkey()), r(t.registry), w(t.vault), w(exit_epoch), r(a(SYSTEM))],
        data: data("close_epoch", &[]),
    };
    t.send("close_epoch", vec![ix], &[&keeper]);

    let mut metas = vec![
        ws(keeper.pubkey()), r(t.registry), w(t.vault), w(exit_epoch), w(t.stock_custody), w(t.usdc_buffer), w(t.redeem_stock),
        w(t.redeem_usdc), r(a(TOKEN)), r(a(TSLA_MINT)), r(a(TOKEN_2022)),
    ];
    metas.extend(t.kamino_block());
    let ix = Instruction { program_id: t.program, accounts: metas, data: data("settle_epoch", &vec_arg(&venue_kamino())) };
    t.send("settle_epoch", vec![ix], &[&keeper]);
    let paid = t.token_amount(&t.redeem_stock);
    eprintln!("stock withdrawn from Kamino for the exit: {} base units", paid);
    assert!(paid >= 99_990_000, "expected ~1 TSLAx back, got {paid}");

    let ix = Instruction {
        program_id: t.program,
        accounts: vec![
            ws(user.pubkey()), w(t.vault), w(exit_request), w(exit_epoch), w(t.share_mint), w(escrow), w(t.redeem_stock),
            w(t.redeem_usdc), w(t.user_stock), w(t.user_usdc), r(a(TOKEN)), r(a(TSLA_MINT)), r(a(TOKEN_2022)),
        ],
        data: data("redeem", &[]),
    };
    t.send("redeem", vec![ix], &[&user]);
    let back = t.token_amount(&t.user_stock);
    eprintln!("user TSLAx after redeem: {}", back);
    assert!(back >= 99_990_000);
}
