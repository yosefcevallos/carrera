use anchor_lang::prelude::*;

#[error_code]
pub enum CarreraError {
    #[msg("Program is paused")]
    Paused,
    #[msg("Signer is not authorised for this instruction")]
    Unauthorized,
    #[msg("Vault is not in the required state")]
    WrongState,
    #[msg("Step counter does not match")]
    WrongStep,
    #[msg("Underlying market is closed")]
    MarketClosed,
    #[msg("Allocation rule does not permit this transition")]
    RuleNotSatisfied,
    #[msg("Not enough funding samples")]
    InsufficientSamples,
    #[msg("NAV cache is stale")]
    NavStale,
    #[msg("Slippage bound exceeded")]
    SlippageExceeded,
    #[msg("Hedge is outside tolerance")]
    HedgeOutOfTolerance,
    #[msg("LTV would exceed the tier limit")]
    LtvTooHigh,
    #[msg("Phoenix margin below minimum")]
    MarginTooLow,
    #[msg("Deposit cap exceeded")]
    DepositCapExceeded,
    #[msg("Epoch is not closed")]
    EpochNotClosed,
    #[msg("Epoch is not settled")]
    EpochNotSettled,
    #[msg("Epoch cannot be funded from current headroom")]
    EpochUnderfunded,
    #[msg("Too soon since the last call")]
    TooSoon,
    #[msg("Venue CPI is not wired in this build")]
    VenueNotWired,
    #[msg("Mock values are not accepted in this build")]
    MockNotAllowed,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Invalid argument")]
    InvalidArgument,
    #[msg("Venue accounts missing from remaining_accounts")]
    VenueAccountsMissing,
    #[msg("Venue account does not match the expected address")]
    VenueAccountsMismatch,
    #[msg("Venue CPI failed or returned less than required")]
    VenueCpiFailed,
}
