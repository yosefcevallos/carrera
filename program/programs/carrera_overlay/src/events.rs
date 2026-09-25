use anchor_lang::prelude::*;

#[event]
pub struct Deposited {
    pub vault: Pubkey,
    pub user: Pubkey,
    pub qty: u64,
    pub shares: u64,
}

#[event]
pub struct ExitRequested {
    pub vault: Pubkey,
    pub user: Pubkey,
    pub nonce: u64,
    pub shares: u64,
    pub epoch_id: u64,
}

#[event]
pub struct ExitCancelled {
    pub vault: Pubkey,
    pub user: Pubkey,
    pub nonce: u64,
    pub shares: u64,
}

#[event]
pub struct EpochClosed {
    pub vault: Pubkey,
    pub epoch_id: u64,
    pub shares_total: u64,
    pub stock_owed: u64,
    pub usdc_owed: u64,
}

#[event]
pub struct EpochSettled {
    pub vault: Pubkey,
    pub epoch_id: u64,
    pub stock_paid: u64,
    pub usdc_paid: u64,
}

#[event]
pub struct Redeemed {
    pub vault: Pubkey,
    pub user: Pubkey,
    pub nonce: u64,
    pub shares: u64,
    pub stock: u64,
    pub usdc: u64,
}

#[event]
pub struct StateChanged {
    pub vault: Pubkey,
    pub from: u8,
    pub to: u8,
    pub step: u8,
}

#[event]
pub struct RuleEvaluated {
    pub vault: Pubkey,
    pub f_avg_bps: i64,
    pub parked_apy_bps: u32,
    pub r_bps: u32,
    pub hurdle_bps: i64,
    pub decision: u8,
}

#[event]
pub struct NavRefreshed {
    pub vault: Pubkey,
    pub nav_usd_e6: u64,
    pub share_price_stock_e6: u64,
    pub price_e6: u64,
    /// Total debt when it is below `DEBT_DUST_USDC` (carried, not settled), else 0.
    pub debt_dust_usdc: u64,
}

#[event]
pub struct FundingRecorded {
    pub vault: Pubkey,
    pub rate_bps_e6_hourly: i64,
    pub f_avg_bps: i64,
    pub samples: u8,
}

#[event]
pub struct KaminoRatesRecorded {
    pub borrow_apy_bps: u32,
    pub supply_apy_bps: u32,
}

#[event]
pub struct Rebalanced {
    pub vault: Pubkey,
    pub kind: u8,
    pub amount: u64,
}

#[event]
pub struct FeeCrystallised {
    pub vault: Pubkey,
    pub shares: u64,
    pub high_water_e6: u64,
}

#[event]
pub struct Paused {}

#[event]
pub struct Unpaused {}
