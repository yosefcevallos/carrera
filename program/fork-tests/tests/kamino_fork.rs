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
const PHOENIX_GLOBAL_CONFIG: &str = "2zskx2iyCvb6Stg7RBZkt1f6MrF4dpYtMG3yMvKwqtUZ";

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
    /// Slots added per funding sample in `drive_to_winding` (seconds are always 3600).
    funding_slot_step: u64,
    /// What the user deposits in `setup_deposited` (base units of TSLAx).
    deposit_qty: u64,
    /// VaultParams bytes `init_vault` gets (tier B defaults; tests may patch fields first).
    params: Vec<u8>,
}

impl World {
    fn new() -> Self {
        Self::new_at(DUMP_TS + 5, DUMP_SLOT, 9000)
    }

    /// Start the fork clock at (`ts`, `slot`); `slot` must not precede the Kamino dump slot
    /// (reserve `last_update.slot`), `ts` may (Scope prices are re-stamped on every warp).
    fn new_at(ts: i64, slot: u64, funding_slot_step: u64) -> Self {
        assert!(slot >= DUMP_SLOT, "fork slot {slot} precedes the Kamino dump slot {DUMP_SLOT}");
        // Signature verification off: the Phoenix onboarding replay carries the exchange's
        // onboarder as a signer whose key the fork does not have.
        let mut svm = LiteSVM::new().with_sigverify(false);
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
            clock_ts: ts,
            clock_slot: slot,
            funding_slot_step,
            deposit_qty: 100_000_000,
            params: vault_params(),
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
        let mut tx = Transaction::new_unsigned(msg);
        // Partial: signer metas the test cannot sign for (Phoenix's onboarder) stay unsigned.
        tx.partial_sign(signers, self.svm.latest_blockhash());
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

/// Vault field offsets after the discriminator (state, step, market_open, collateral_qty,
/// basis_spot_qty, debt_usdc, debt_b_usdc, parked_usdc, ...).
struct Offsets {
    collateral: usize,
    basis_spot: usize,
    debt: usize,
    parked: usize,
}

fn offsets() -> Offsets {
    let collateral = off::after_params(vault_params().len()) + 3;
    Offsets { collateral, basis_spot: collateral + 8, debt: collateral + 16, parked: collateral + 32 }
}

fn refresh_ix(t: &World) -> Instruction {
    Instruction {
        program_id: t.program,
        accounts: vec![rs(t.keeper.pubkey()), r(t.registry), w(t.vault), w(a(RESERVE_TSLA)), r(a(KLEND)), r(a(MARKET)), r(a(SCOPE))],
        data: data("refresh_nav", &[0]), // None
    }
}

/// init registry + vault → deposit 1 TSLAx → init_kamino_obligation → sync_collateral →
/// on-chain rates + price → 24 funding samples → market open → wind_start (Kamino borrow).
/// Leaves the vault Winding at step 0 with the borrowed USDC in the buffer.
fn drive_to_winding(t: &mut World) {
    setup_deposited(t);
    warm_and_wind(t);
}

/// The first half of `drive_to_winding`: everything up to and including `sync_collateral`.
fn setup_deposited(t: &mut World) {
    let params = t.params.clone();
    let o = offsets();
    let (o_collateral, o_parked) = (o.collateral, o.parked);

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
    acc.data[64..72].copy_from_slice(&t.deposit_qty.to_le_bytes());
    t.svm.set_account(t.user_stock, acc).unwrap();
    assert_eq!(t.token_amount(&t.user_stock), t.deposit_qty);

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

    let refresh = refresh_ix;
    let ix = refresh(t);
    t.send("refresh_nav", vec![ix], &[&keeper]);
    // parked(8), phoenix_equity(8), phoenix_short(8), funding[24](192), head(1), samples(1), last_ts(8), nav(8), share_price(8) → price_e6
    let o_price = o_parked + 8 + 8 + 8 + 8 * 24 + 1 + 1 + 8 + 8 + 8;
    let price = t.vault_field_u64(o_price);
    eprintln!("on-chain TSLAx price: {} USD", price as f64 / 1e6);
    assert!((100_000_000..1_000_000_000).contains(&price));

    // ---- deposit 1 TSLAx into custody
    let mut dep = t.deposit_qty.to_le_bytes().to_vec();
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
    assert_eq!(t.token_amount(&t.stock_custody), t.deposit_qty);
    assert_eq!(t.vault_field_u64(o_collateral), t.deposit_qty);

    // ---- init the Kamino obligation (user metadata + obligation) via CPI
    let mut metas = vec![ws(keeper.pubkey()), r(t.registry), w(t.vault), r(a(SYSTEM)), r(a(RENT))];
    metas.extend(t.kamino_block());
    metas.push(w(t.user_meta));
    let ix = Instruction { program_id: t.program, accounts: metas, data: data("init_kamino_obligation", &vec_arg(&venue_kamino())) };
    t.send("init_kamino_obligation", vec![ix], &[&keeper]);
    assert!(t.svm.get_account(&t.obligation).map(|x| x.data.len() > 8).unwrap_or(false), "obligation created");

    // ---- sync custody into Kamino as collateral (idempotent: a second call with empty custody is a no-op)
    let ix = t.keeper_vault("sync_collateral", &[], true);
    t.send("sync_collateral", vec![ix], &[&keeper]);
    assert_eq!(t.token_amount(&t.stock_custody), 0, "custody moved into the obligation");
    let ix = t.keeper_vault("sync_collateral", &[], true);
    t.send("sync_collateral (again, custody empty)", vec![ix], &[&keeper]);
    {
        // Where does klend store the deposit reserve key? (documents the obligation layout the program scans)
        let ob = t.svm.get_account(&t.obligation).unwrap().data;
        let key = a(RESERVE_TSLA);
        let hits: Vec<usize> = (0..ob.len() - 32).filter(|&o| &ob[o..o + 32] == key.as_ref()).collect();
        eprintln!("obligation: {} bytes, stock reserve key at offsets {:?} (expected 8 + 88 = 96)", ob.len(), hits);
        assert!(hits.contains(&96), "deposit reserve not at the expected offset");
    }

}

/// The second half of `drive_to_winding`: 24 funding samples, market open, fresh NAV, `wind_start`.
fn warm_and_wind(t: &mut World) {
    let o = offsets();
    let (o_debt, o_parked) = (o.debt, o.parked);
    let _ = o_parked;
    let keeper = t.keeper.insecure_clone();
    let refresh = refresh_ix;
    // ---- 24 funding samples (non-mock spacing is 59 min) and market open
    for i in 0..24 {
        t.set_clock(3600, t.funding_slot_step);
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
    let ix = refresh(t);
    t.send("refresh_nav (fresh)", vec![ix], &[&keeper]);

    // ---- wind_start from Idle: Kamino borrow of D = 30% × collateral value into usdc_buffer
    let ix = t.keeper_vault("wind_start", &[], true);
    t.send("wind_start", vec![ix], &[&keeper]);
    let debt = t.vault_field_u64(o_debt);
    let buffer = t.token_amount(&t.usdc_buffer);
    eprintln!("borrowed D = {} USDC (buffer holds {} after Kamino's origination fee)", debt as f64 / 1e6, buffer as f64 / 1e6);
    assert!(debt > 0 && buffer > 0 && buffer <= debt);
    assert_eq!(t.vault_state(), 2, "Winding");
}

#[test]
fn kamino_legs_end_to_end() {
    let mut t = World::new();
    drive_to_winding(&mut t);
    let o = offsets();
    let o_debt = o.debt;
    let admin = t.admin.insecure_clone();
    let keeper = t.keeper.insecure_clone();
    let user = t.user.insecure_clone();
    let refresh = refresh_ix;

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

// ---------------------------------------------------------------------------------------------
// Jupiter: wind_step(1) with a live route dumped by `keeper venues prove-jupiter`.
// ---------------------------------------------------------------------------------------------

/// The route `keeper venues prove-jupiter --vault TSLA` dumped: instruction data, the block of
/// account metas exactly as the keeper appends them, and the accounts/programs it touches.
struct JupiterScenario {
    data: Vec<u8>,
    block: Vec<AccountMeta>,
    reverse_data: Vec<u8>,
    reverse_block: Vec<AccountMeta>,
    programs: Vec<String>,
    dump_slot: u64,
    quoted_out: u64,
}

fn load_jupiter_scenario() -> JupiterScenario {
    let dir = fixtures_dir().join("jupiter");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("scenario.json")).expect("scenario.json; run `keeper venues prove-jupiter --vault TSLA`")).unwrap();
    JupiterScenario {
        data: b64(v["forward"]["data_base64"].as_str().unwrap()),
        block: metas_from(&v["forward"]["block"]),
        reverse_data: b64(v["reverse"]["data_base64"].as_str().unwrap()),
        reverse_block: metas_from(&v["reverse"]["block"]),
        programs: v["programs"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect(),
        dump_slot: v["dump_slot"].as_u64().unwrap(),
        quoted_out: v["forward"]["quoted_out_amount"].as_u64().unwrap(),
    }
}

/// VenueData { blocks: KAMINO | JUPITER, zeros..., jupiter_data }
fn venue_kamino_jupiter(route: &[u8]) -> Vec<u8> {
    let mut b = vec![1u8 | 4, 0, 0];
    b.extend_from_slice(&[0u8; 8 * 5]);
    b.extend(vec_arg(route));
    b
}

#[test]
fn jupiter_wind_step_one_in_fork() {
    let scen = load_jupiter_scenario();
    let mut t = World::new();
    t.load_fixture_dir("jupiter", &scen.programs);
    drive_to_winding(&mut t);
    let o = offsets();
    let keeper = t.keeper.insecure_clone();
    eprintln!(
        "fork clock slot {} vs route dump slot {} ({} slots apart); route quoted {} TSLAx base units per 1 USDC",
        t.clock_slot,
        scen.dump_slot,
        t.clock_slot as i64 - scen.dump_slot as i64,
        scen.quoted_out
    );

    let buffer_before = t.token_amount(&t.usdc_buffer);
    let spot_before = t.vault_field_u64(o.basis_spot);
    let debt = t.vault_field_u64(o.debt);
    assert_eq!(spot_before, 0);

    // wind_step(1): withdraw nothing (the buffer holds D), swap USDC → TSLAx through the route,
    // deposit the TSLAx into the Kamino obligation as collateral.
    let mut metas = vec![ws(keeper.pubkey()), w(t.registry), w(t.vault)];
    metas.extend(t.kamino_block());
    metas.extend(scen.block.iter().cloned());
    let mut args = vec![1u8];
    args.extend(vec_arg(&venue_kamino_jupiter(&scen.data)));
    let ix = Instruction { program_id: t.program, accounts: metas, data: data("wind_step", &args) };
    let logs = t.send("wind_step 1 (Jupiter)", vec![ix], &[&keeper]);
    for l in logs.iter().filter(|l| l.contains("SwapEvent") || l.contains("Program log: Instruction") || l.contains("consumed")) {
        eprintln!("    {l}");
    }

    let buffer_after = t.token_amount(&t.usdc_buffer);
    let spot_after = t.vault_field_u64(o.basis_spot);
    let step = t.svm.get_account(&t.vault).unwrap().data[o.collateral - 2];
    eprintln!(
        "buffer {} → {} USDC, basis_spot_qty {} → {} TSLAx base units, debt {} USDC, custody after deposit {}",
        buffer_before as f64 / 1e6,
        buffer_after as f64 / 1e6,
        spot_before,
        spot_after,
        debt as f64 / 1e6,
        t.token_amount(&t.stock_custody)
    );
    assert_eq!(t.vault_state(), 2, "still Winding");
    assert_eq!(step, 1, "step advanced to 1");
    assert!(buffer_after < buffer_before / 100, "the buffer was swapped out");
    assert!(spot_after > 0, "TSLAx bought");
    // Sanity on the fill versus the route's 1-USDC quote: within 5% per USDC (size + time drift).
    let per_usdc = spot_after as f64 / (buffer_before as f64 / 1e6);
    let quote = scen.quoted_out as f64;
    assert!((per_usdc - quote).abs() / quote < 0.05, "fill {per_usdc} vs quote {quote}");
    assert_eq!(t.token_amount(&t.stock_custody), 0, "the bought TSLAx went into the obligation");
}

// ---------------------------------------------------------------------------------------------
// Phoenix: the whole basis cycle with the mainnet Phoenix + Ember accounts dumped by
// `keeper venues prove-phoenix` and both Jupiter routes from `keeper venues prove-jupiter`.
// ---------------------------------------------------------------------------------------------

struct PhoenixScenario {
    block: Vec<AccountMeta>,
    gti: u8,
    atb: u8,
    base_lot_size: u64,
    trader_account: Address,
    trader_token: Address,
    canonical_mint: Address,
    create_ata_ix: Instruction,
    /// Phoenix's own register + delegated-onboarding instructions for the vault's trader.
    onboarding_ixs: Vec<Instruction>,
    trader_onboarder: Address,
    keeper: Address,
    programs: Vec<String>,
    dump_slot: u64,
    dump_ts: i64,
}

fn metas_from(v: &serde_json::Value) -> Vec<AccountMeta> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|m| AccountMeta { pubkey: a(m["pubkey"].as_str().unwrap()), is_signer: m["is_signer"].as_bool().unwrap(), is_writable: m["is_writable"].as_bool().unwrap() })
        .collect()
}

fn ix_from(v: &serde_json::Value) -> Instruction {
    Instruction { program_id: a(v["program_id"].as_str().unwrap()), accounts: metas_from(&v["accounts"]), data: b64(v["data_base64"].as_str().unwrap()) }
}

fn load_phoenix_scenario() -> PhoenixScenario {
    let dir = fixtures_dir().join("phoenix");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("scenario.json")).expect("scenario.json; run `keeper venues prove-phoenix --vault TSLA`")).unwrap();
    PhoenixScenario {
        block: metas_from(&v["block"]),
        gti: v["phoenix_gti"].as_u64().unwrap() as u8,
        atb: v["phoenix_atb"].as_u64().unwrap() as u8,
        base_lot_size: v["base_lot_size"].as_u64().unwrap(),
        trader_account: a(v["trader_account"].as_str().unwrap()),
        trader_token: a(v["trader_token_account"].as_str().unwrap()),
        canonical_mint: a(v["canonical_mint"].as_str().unwrap()),
        create_ata_ix: ix_from(&v["create_ata_ix"]),
        onboarding_ixs: v["onboarding_ixs"].as_array().unwrap().iter().map(ix_from).collect(),
        trader_onboarder: a(v["trader_onboarder"].as_str().unwrap()),
        keeper: a(v["keeper"].as_str().unwrap()),
        programs: v["programs"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect(),
        dump_slot: v["dump_slot"].as_u64().unwrap(),
        dump_ts: v["dump_ts"].as_i64().unwrap(),
    }
}

/// Borsh `VenueData` with every field.
#[allow(clippy::too_many_arguments)]
fn venue_data(blocks: u8, gti: u8, atb: u8, base_lot_size: u64, last_valid_slot: u64, equity: u64, client_order_id: u64, jupiter: &[u8]) -> Vec<u8> {
    let mut b = vec![blocks, gti, atb];
    b.extend_from_slice(&base_lot_size.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes()); // price_in_ticks: market
    b.extend_from_slice(&last_valid_slot.to_le_bytes());
    b.extend_from_slice(&equity.to_le_bytes());
    b.extend_from_slice(&client_order_id.to_le_bytes());
    b.extend(vec_arg(jupiter));
    b
}

impl World {
    fn load_fixture_dir(&mut self, sub: &str, programs: &[String]) {
        let dir = fixtures_dir().join(sub);
        let skip = [self.vault, self.stock_custody, self.usdc_buffer];
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            if !name.starts_with("acct_") {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let pubkey = a(v["pubkey"].as_str().unwrap());
            if skip.contains(&pubkey) {
                continue;
            }
            let account = Account {
                lamports: v["lamports"].as_u64().unwrap(),
                data: b64(v["data_base64"].as_str().unwrap()),
                owner: a(v["owner"].as_str().unwrap()),
                executable: false,
                rent_epoch: 0,
            };
            self.svm.set_account(pubkey, account).unwrap();
        }
        for name in programs {
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap();
            let elf = b64(v["data_base64"].as_str().unwrap());
            self.svm.add_program(a(v["pubkey"].as_str().unwrap()), trim_elf(&elf)).unwrap();
        }
    }

    /// The vault's Phoenix trader collateral (`TraderHeader.trader_state.quote_lot_collateral`:
    /// i64 after discriminant 8, SequenceNumber 16, key 32, authority 32 → offset 88), in quote lots.
    fn trader_collateral(&self, trader_account: &Address) -> i64 {
        let acc = self.svm.get_account(trader_account).expect("trader account");
        i64::from_le_bytes(acc.data[88..96].try_into().unwrap())
    }

    /// A crank with the Kamino block, the Phoenix block when `phoenix`, and a Jupiter route when
    /// given. Each step gets only the blocks it consumes: the three-block shape has ~85 unique
    /// accounts, over LiteSVM's 64 account locks (mainnet allows 128).
    fn crank_kp(&self, name: &str, args: &[u8], ph: &PhoenixScenario, phoenix: bool, equity: u64, order_id: u64, jupiter: Option<(&[u8], &[AccountMeta])>) -> Instruction {
        let mut metas = vec![ws(self.keeper.pubkey()), w(self.registry), w(self.vault)];
        metas.extend(self.kamino_block());
        let mut blocks = 1u8;
        if phoenix {
            metas.extend(ph.block.iter().cloned());
            blocks |= 2;
        }
        let mut route: &[u8] = &[];
        if let Some((data, block)) = jupiter {
            metas.extend(block.iter().cloned());
            blocks |= 4;
            route = data;
        }
        let mut d = args.to_vec();
        d.extend(vec_arg(&venue_data(blocks, ph.gti, ph.atb, ph.base_lot_size, self.clock_slot + 150, equity, order_id, route)));
        Instruction { program_id: self.program, accounts: metas, data: data(name, &d) }
    }
}

#[test]
fn phoenix_basis_cycle_in_fork() {
    let jup = load_jupiter_scenario();
    let ph = load_phoenix_scenario();
    // Phoenix validates its oracle and spline state against the clock, so the fork runs the
    // venue steps at the Phoenix dump slot, a few seconds after the dump: the 24 funding samples
    // before them advance the clock by seconds only, starting 24 h earlier. (Whirlpool refuses a
    // clock behind its last update, so the Jupiter routes are dumped before the Phoenix accounts.)
    let mut t = World::new_at(ph.dump_ts - 24 * 3600 + 5, ph.dump_slot - 24, 1);
    // Jupiter's xStock pools traded ~1.5% under the Kamino (Scope) oracle when the routes were
    // dumped, so the sell leg needs a wider `max_swap_slippage_bps` than tier B's 50 to clear
    // the program's oracle floor. (Buys pass at any discount: they deliver more stock.)
    t.params[41..45].copy_from_slice(&300u32.to_le_bytes());
    // A small deposit (0.034 TSLAx), as on mainnet on 25 Sep 2026: the spot leg is ~1.0M base
    // units, ten lots plus a ~2.8 % remainder that no whole-lot short can cover.
    t.deposit_qty = 3_400_000;
    t.load_fixture_dir("jupiter", &jup.programs);
    t.load_fixture_dir("phoenix", &ph.programs);
    // Phoenix only trades when the cluster's LastRestartSlot sysvar matches the restart slot the
    // exchange acknowledged (GlobalConfigPrefixRaw.acknowledged_restart_slot at offset 1096).
    {
        let cfg = t.svm.get_account(&a(PHOENIX_GLOBAL_CONFIG)).expect("global config fixture");
        let ack = u64::from_le_bytes(cfg.data[1096..1104].try_into().unwrap());
        t.svm.set_sysvar(&solana_last_restart_slot::LastRestartSlot { last_restart_slot: ack });
        eprintln!("Phoenix acknowledged restart slot {ack}; exchange status byte {}", cfg.data[8 + 32 + 256 + 32 * 5 + 16 + 32]);
    }
    setup_deposited(&mut t);
    // Half the shares ask to leave before the position is built: the epoch closes once the 24 h
    // of funding samples have passed, and the exit is paid through a partial release in Basis.
    let user = t.user.insecure_clone();
    let exit_request = pda(&[b"exit", t.vault.as_ref(), user.pubkey().as_ref(), &1u64.to_le_bytes()], &t.program);
    let exit_epoch = pda(&[b"epoch", t.vault.as_ref(), &0u64.to_le_bytes()], &t.program);
    let escrow = pda(&[b"escrow", t.vault.as_ref()], &t.program);
    {
        let mut req = (t.deposit_qty / 2).to_le_bytes().to_vec();
        req.extend_from_slice(&1u64.to_le_bytes());
        let ix = Instruction {
            program_id: t.program,
            accounts: vec![ws(user.pubkey()), r(t.registry), w(t.vault), w(exit_request), w(exit_epoch), r(t.share_mint), w(t.user_shares), w(escrow), r(a(TOKEN)), r(a(SYSTEM))],
            data: data("request_exit", &req),
        };
        t.send("request_exit (half)", vec![ix], &[&user]);
    }
    warm_and_wind(&mut t);
    assert_eq!(t.clock_slot, ph.dump_slot, "venue steps run at the Phoenix dump slot");
    let debt_initial = t.vault_field_u64(offsets().debt);
    let o = offsets();
    let keeper = t.keeper.insecure_clone();
    let admin = t.admin.insecure_clone();
    let o_debt_b = o.debt + 8;
    let o_equity = o.parked + 8;
    let o_short = o.parked + 16;

    // ---- one-time setup the keeper does on first sight of the vault: collateral ATA + register_trader
    let payer_swap = |ix: &Instruction, from: &Address, to: &Address| Instruction {
        program_id: ix.program_id,
        accounts: ix.accounts.iter().map(|m| AccountMeta { pubkey: if m.pubkey == *from { *to } else { m.pubkey }, ..m.clone() }).collect(),
        data: ix.data.clone(),
    };
    // (Phoenix's onboarder signs on mainnet through `send-register-ixs`; here its signature is skipped.)
    let mut setup = vec![payer_swap(&ph.create_ata_ix, &ph.keeper, &keeper.pubkey())];
    setup.extend(ph.onboarding_ixs.iter().map(|ix| payer_swap(ix, &ph.keeper, &keeper.pubkey())));
    assert!(setup.iter().any(|ix| ix.accounts.iter().any(|m| m.pubkey == ph.trader_onboarder && m.is_signer)), "onboarder signs the delegated onboarding");
    t.svm.airdrop(&ph.trader_onboarder, 1_000_000_000).unwrap();
    t.send("create collateral ATA + register_trader + onboard_trader_delegated", setup, &[&keeper]);
    assert!(t.svm.get_account(&ph.trader_account).map(|x| x.data.len() > 96).unwrap_or(false), "trader registered");
    let flags = u32::from_le_bytes(t.svm.get_account(&ph.trader_account).unwrap().data[96..100].try_into().unwrap());
    eprintln!("trader capability flags after onboarding: {flags:#x}");
    assert_eq!(flags & 0b111110, 0b111110, "limit, market, risk-increase, deposit, withdraw enabled");
    assert_eq!(t.trader_collateral(&ph.trader_account), 0);
    assert_eq!(t.token_amount(&ph.trader_token), 0);
    let _ = ph.canonical_mint;

    // ---- wind 1: USDC → TSLAx (Jupiter) → Kamino collateral
    let ix = t.crank_kp("wind_step", &[1], &ph, false, 0, 1, Some((&jup.data, &jup.block)));
    t.send("wind_step 1 (Jupiter)", vec![ix], &[&keeper]);
    let spot = t.vault_field_u64(o.basis_spot);
    assert!(spot > 0);

    // ---- wind 2: Kamino borrow D_b → Ember wrap → Phoenix deposit
    t.set_clock(1, 1);
    let ix = t.crank_kp("wind_step", &[2], &ph, true, 0, 2, None);
    let logs = t.send("wind_step 2 (Kamino borrow + Ember + Phoenix deposit)", vec![ix], &[&keeper]);
    for l in logs.iter().filter(|l| l.contains("consumed") && (l.contains("Etrn") || l.contains("EMBER"))) {
        eprintln!("    {l}");
    }
    let debt_b = t.vault_field_u64(o_debt_b);
    let collateral = t.trader_collateral(&ph.trader_account);
    eprintln!("D_b borrowed {} USDC → Phoenix trader collateral {} quote lots (token account {} left)", debt_b as f64 / 1e6, collateral, t.token_amount(&ph.trader_token));
    assert!(debt_b > 0);
    assert_eq!(collateral as u64, debt_b, "deposit landed as collateral (quote lot = 1 USDC base unit)");
    assert_eq!(t.vault_field_u64(o_equity), debt_b);

    // ---- wind 3: IOC short of basis_spot_qty (whole base lots), all-or-nothing
    t.set_clock(1, 1);
    let ix = t.crank_kp("wind_step", &[3], &ph, true, 0, 3, None);
    let logs = t.send("wind_step 3 (Phoenix short)", vec![ix], &[&keeper]);
    for l in logs.iter().filter(|l| l.contains("consumed") && l.contains("Etrn")) {
        eprintln!("    {l}");
    }
    let short = t.vault_field_u64(o_short);
    let lots = spot / ph.base_lot_size;
    let remainder = spot - lots * ph.base_lot_size;
    eprintln!("short {} base units = {} lots against {} spot base units ({} unhedged remainder, {} bps); collateral now {} quote lots", short, short / ph.base_lot_size, spot, remainder, remainder as u128 * 10_000 / spot as u128, t.trader_collateral(&ph.trader_account));
    assert_eq!(short, lots * ph.base_lot_size, "filled the whole rounded-down size");
    assert!(short > 0);
    assert!(remainder as u128 * 10_000 / spot as u128 > 30, "the remainder exceeds max_perp_slippage_bps, so only a lot-aware fill check passes");

    // ---- commit: equity as the keeper would read it (D6): the trader account's collateral after fees
    t.set_clock(1, 1);
    let equity = t.trader_collateral(&ph.trader_account) as u64;
    assert!(equity < debt_b && equity > debt_b * 99 / 100, "taker fee came out of the collateral: {equity} of {debt_b}");
    let ix = t.crank_kp("wind_commit", &[], &ph, true, equity, 4, None);
    t.send("wind_commit", vec![ix], &[&keeper]);
    assert_eq!(t.vault_state(), 3, "Basis");
    assert_eq!(t.vault_field_u64(o_equity), equity);

    // ---- partial release for the pending exit: half the short, half the equity, half the spot
    t.set_clock(1, 1);
    let ix = refresh_ix(&t);
    t.send("refresh_nav (epoch)", vec![ix], &[&keeper]);
    let ix = Instruction { program_id: t.program, accounts: vec![ws(keeper.pubkey()), r(t.registry), w(t.vault), w(exit_epoch), r(a(SYSTEM))], data: data("close_epoch", &[]) };
    t.send("close_epoch", vec![ix], &[&keeper]);
    let mut start = 5000u32.to_le_bytes().to_vec();
    start.push(1); // ExitDemand
    let ix = Instruction { program_id: t.program, accounts: vec![ws(keeper.pubkey()), w(t.registry), w(t.vault)], data: data("unwind_partial_start", &start) };
    t.send("unwind_partial_start(5000, exit_demand)", vec![ix], &[&keeper]);
    assert_eq!(t.vault_state(), 6, "PartialUnwinding");
    let spot_full = t.vault_field_u64(o.basis_spot);
    let short_full = t.vault_field_u64(o_short);
    let frac = |n: u8| {
        let mut d = vec![n];
        d.extend_from_slice(&5000u32.to_le_bytes());
        d
    };
    t.set_clock(1, 1);
    let ix = t.crank_kp("unwind_partial_step", &frac(1), &ph, true, 0, 11, None);
    t.send("unwind_partial_step 1 (close half the short)", vec![ix], &[&keeper]);
    assert_eq!(t.vault_field_u64(o_short), short_full - (short_full / 2 / ph.base_lot_size) * ph.base_lot_size, "half the short closed, whole lots");
    t.set_clock(1, 1);
    let live = t.trader_collateral(&ph.trader_account) as u64;
    let ix = t.crank_kp("unwind_partial_step", &frac(2), &ph, true, live, 12, None);
    t.send("unwind_partial_step 2 (withdraw half the equity, repay half of D_b)", vec![ix], &[&keeper]);
    let debt_b_half = t.vault_field_u64(o_debt_b);
    assert!(debt_b_half > debt_b * 45 / 100 && debt_b_half < debt_b * 55 / 100, "D_b halved: {debt_b_half} of {debt_b}");
    t.set_clock(1, 1);
    let ix = t.crank_kp("unwind_partial_step", &frac(3), &ph, false, 0, 13, Some((&jup.reverse_data, &jup.reverse_block)));
    t.send("unwind_partial_step 3 (sell half the spot)", vec![ix], &[&keeper]);
    assert_eq!(t.vault_field_u64(o.basis_spot), spot_full - spot_full / 2);
    // The commit's hedge check needs the lot size the keeper always passes in VenueData.
    let ix = t.crank_kp("unwind_partial_commit", &[], &ph, false, 0, 15, None);
    t.send("unwind_partial_commit", vec![ix], &[&keeper]);
    assert_eq!(t.vault_state(), 3, "back in Basis");
    let parked_after_partial = t.vault_field_u64(o.parked);
    let debt_after_partial = t.vault_field_u64(o.debt);
    let debt = debt_initial;
    eprintln!("partial release: short {} → {}, spot {} → {}, D_b {} → {}, D {} → {} USDC, parked {} USDC supplied", short_full, t.vault_field_u64(o_short), spot_full, t.vault_field_u64(o.basis_spot), debt_b, debt_b_half, debt as f64 / 1e6, debt_after_partial as f64 / 1e6, parked_after_partial as f64 / 1e6);
    assert!(debt_after_partial > debt * 45 / 100 && debt_after_partial < debt * 55 / 100, "the leaving half's share of D repaid: {debt_after_partial} of {debt}");

    // ---- settle the exit epoch from Basis: half a TSLAx comes out of the obligation
    t.set_clock(1, 1);
    let ix = refresh_ix(&t);
    t.send("refresh_nav (settle)", vec![ix], &[&keeper]);
    let mut metas = vec![
        ws(keeper.pubkey()), r(t.registry), w(t.vault), w(exit_epoch), w(t.stock_custody), w(t.usdc_buffer), w(t.redeem_stock), w(t.redeem_usdc), r(a(TOKEN)),
        r(a(TSLA_MINT)), r(a(TOKEN_2022)),
    ];
    metas.extend(t.kamino_block());
    metas.extend(ph.block.iter().cloned());
    let equity_now = t.trader_collateral(&ph.trader_account) as u64;
    let ix = Instruction { program_id: t.program, accounts: metas, data: data("settle_epoch", &vec_arg(&venue_data(1 | 2, ph.gti, ph.atb, ph.base_lot_size, t.clock_slot + 150, equity_now, 14, &[]))) };
    t.send("settle_epoch (Basis, half the shares)", vec![ix], &[&keeper]);
    let paid = t.token_amount(&t.redeem_stock);
    eprintln!("exit settled in Basis: {} base units to redeem_stock, {} USDC", paid, t.token_amount(&t.redeem_usdc) as f64 / 1e6);
    assert!(paid >= t.deposit_qty / 2 - 10_000, "expected ~half the deposit for half the shares, got {paid}");
    let equity = t.trader_collateral(&ph.trader_account) as u64;
    let debt_b = debt_b_half;

    // ---- guardian emergency unwind: close the short (reduce-only IOC), withdraw, sell, commit
    let ix = Instruction { program_id: t.program, accounts: vec![ws(admin.pubkey()), w(t.registry), w(t.vault)], data: data("unwind_start", &[2]) };
    t.send("unwind_start (guardian, emergency)", vec![ix], &[&admin]);
    assert_eq!(t.vault_state(), 4, "Unwinding");

    t.set_clock(1, 1);
    let ix = t.crank_kp("unwind_step", &[1], &ph, true, equity, 5, None);
    t.send("unwind_step 1 (close short)", vec![ix], &[&keeper]);
    assert_eq!(t.vault_field_u64(o_short), 0, "short closed");
    let after_close = t.trader_collateral(&ph.trader_account);
    eprintln!("collateral after close: {} quote lots (realised PnL + fees vs {} deposited)", after_close, debt_b);
    assert!(after_close > 0);

    t.set_clock(1, 1);
    let equity = after_close as u64;
    let buffer_before = t.token_amount(&t.usdc_buffer);
    let ix = t.crank_kp("unwind_step", &[2], &ph, true, equity, 6, None);
    t.send("unwind_step 2 (Phoenix withdraw + Ember unwrap + Kamino repay)", vec![ix], &[&keeper]);
    assert_eq!(t.trader_collateral(&ph.trader_account), 0, "all collateral withdrawn");
    assert_eq!(t.token_amount(&ph.trader_token), 0, "unwrapped back to USDC");
    let debt_b_after = t.vault_field_u64(o_debt_b);
    eprintln!("after withdraw: D_b {} → {} USDC, buffer {} → {}", debt_b as f64 / 1e6, debt_b_after as f64 / 1e6, buffer_before as f64 / 1e6, t.token_amount(&t.usdc_buffer) as f64 / 1e6);
    // The settled exit's USDC leg came out of the Phoenix equity, so what is left repays most of
    // D_b; the residual folds into the primary loan at unwind_commit.
    assert!(debt_b_after < debt_b / 10, "D_b mostly repaid: {debt_b_after} left of {debt_b}");

    t.set_clock(1, 1);
    let ix = t.crank_kp("unwind_step", &[3], &ph, false, 0, 7, Some((&jup.reverse_data, &jup.reverse_block)));
    t.send("unwind_step 3 (Kamino withdraw + Jupiter sell)", vec![ix], &[&keeper]);
    assert_eq!(t.vault_field_u64(o.basis_spot), 0);
    let parked = t.vault_field_u64(o.parked);
    eprintln!("sold the basis spot back: parked {} USDC", parked as f64 / 1e6);
    assert!(parked > 0);

    let ix = t.keeper_vault("unwind_commit", &[], true);
    t.send("unwind_commit", vec![ix], &[&keeper]);
    assert_eq!(t.vault_state(), 1, "Parked");
    assert_eq!(t.vault_field_u64(o_debt_b), 0);
    let debt = t.vault_field_u64(o.debt);
    eprintln!("Parked: debt {} USDC, parked {} USDC (round trip cost {} USDC)", debt as f64 / 1e6, t.vault_field_u64(o.parked) as f64 / 1e6, (debt as i64 - t.vault_field_u64(o.parked) as i64) as f64 / 1e6);
}

// ---------------------------------------------------------------------------------------------
// Migration window: a vault upgraded to the real build whose stock is still in custody (no
// obligation yet, nothing synced) must still settle exits, from custody.
// ---------------------------------------------------------------------------------------------

#[test]
fn exit_settles_from_custody_without_obligation() {
    let mut t = World::new();
    let params = vault_params();
    let mut init_reg = Vec::new();
    init_reg.extend_from_slice(t.admin.pubkey().as_ref());
    init_reg.extend_from_slice(&1u32.to_le_bytes());
    init_reg.extend_from_slice(t.keeper.pubkey().as_ref());
    let admin = t.admin.insecure_clone();
    let keeper = t.keeper.insecure_clone();
    let user = t.user.insecure_clone();
    let ix = Instruction { program_id: t.program, accounts: vec![ws(admin.pubkey()), w(t.registry), r(a(USDC_MINT)), r(a(SYSTEM))], data: data("init_registry", &init_reg) };
    t.send("init_registry", vec![ix], &[&admin]);
    let mut init_vault = vec![1u8];
    init_vault.extend_from_slice(&params);
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![
            ws(admin.pubkey()), r(t.registry), w(t.vault), r(a(TSLA_MINT)), w(t.share_mint), w(t.stock_custody), w(t.usdc_buffer), w(t.redeem_stock),
            w(t.redeem_usdc), r(a(USDC_MINT)), r(a(TOKEN)), r(a(SYSTEM)), r(a(RENT)), r(a(TOKEN_2022)),
        ],
        data: data("init_vault", &init_vault),
    };
    t.send("init_vault", vec![ix], &[&admin]);
    let mut acc = load_fixture("tslax_liq_supply").account;
    acc.data[32..64].copy_from_slice(user.pubkey().as_ref());
    acc.data[64..72].copy_from_slice(&100_000_000u64.to_le_bytes());
    t.svm.set_account(t.user_stock, acc).unwrap();
    t.send(
        "atas",
        vec![
            create_ata_ix(&user.pubkey(), &user.pubkey(), &t.share_mint, &a(TOKEN)),
            create_ata_ix(&user.pubkey(), &user.pubkey(), &a(USDC_MINT), &a(TOKEN)),
            create_ata_ix(&user.pubkey(), &t.vault, &a(USDC_COLL_MINT), &a(TOKEN)),
        ],
        &[&user],
    );
    let ix = refresh_ix(&t);
    t.send("refresh_nav", vec![ix], &[&keeper]);
    let mut dep = 100_000_000u64.to_le_bytes().to_vec();
    dep.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![ws(user.pubkey()), r(t.registry), w(t.vault), w(t.share_mint), r(a(TSLA_MINT)), w(t.stock_custody), w(t.user_stock), w(t.user_shares), r(a(TOKEN)), r(a(TOKEN_2022))],
        data: data("deposit", &dep),
    };
    t.send("deposit", vec![ix], &[&user]);
    assert_eq!(t.token_amount(&t.stock_custody), 100_000_000);
    // No init_kamino_obligation, no sync_collateral: the obligation PDA does not exist.
    assert!(t.svm.get_account(&t.obligation).map(|x| x.data.is_empty()).unwrap_or(true));

    let mut req = 40_000_000u64.to_le_bytes().to_vec();
    req.extend_from_slice(&1u64.to_le_bytes());
    let exit_request = pda(&[b"exit", t.vault.as_ref(), user.pubkey().as_ref(), &1u64.to_le_bytes()], &t.program);
    let exit_epoch = pda(&[b"epoch", t.vault.as_ref(), &0u64.to_le_bytes()], &t.program);
    let escrow = pda(&[b"escrow", t.vault.as_ref()], &t.program);
    let ix = Instruction {
        program_id: t.program,
        accounts: vec![ws(user.pubkey()), r(t.registry), w(t.vault), w(exit_request), w(exit_epoch), r(t.share_mint), w(t.user_shares), w(escrow), r(a(TOKEN)), r(a(SYSTEM))],
        data: data("request_exit", &req),
    };
    t.send("request_exit", vec![ix], &[&user]);
    t.set_clock(3601, 9000);
    let ix = refresh_ix(&t);
    t.send("refresh_nav (epoch)", vec![ix], &[&keeper]);
    let ix = Instruction { program_id: t.program, accounts: vec![ws(keeper.pubkey()), r(t.registry), w(t.vault), w(exit_epoch), r(a(SYSTEM))], data: data("close_epoch", &[]) };
    t.send("close_epoch", vec![ix], &[&keeper]);
    let mut metas = vec![
        ws(keeper.pubkey()), r(t.registry), w(t.vault), w(exit_epoch), w(t.stock_custody), w(t.usdc_buffer), w(t.redeem_stock), w(t.redeem_usdc), r(a(TOKEN)),
        r(a(TSLA_MINT)), r(a(TOKEN_2022)),
    ];
    metas.extend(t.kamino_block());
    let ix = Instruction { program_id: t.program, accounts: metas, data: data("settle_epoch", &vec_arg(&venue_kamino())) };
    t.send("settle_epoch (from custody, no obligation)", vec![ix], &[&keeper]);
    let paid = t.token_amount(&t.redeem_stock);
    eprintln!("settled from custody without an obligation: {} base units moved to redeem_stock", paid);
    assert!(paid >= 39_990_000, "expected ~0.4 TSLAx from custody, got {paid}");
    assert_eq!(t.token_amount(&t.stock_custody), 100_000_000 - paid);
}
