// Vault parameters per spec §6.1 (tiers) and §11 (defaults), plus DECISIONS.md D2/D3.
import anchor from "@coral-xyz/anchor";
const { BN } = anchor;

export type TierName = "A" | "B" | "C" | "D";

const TIER = {
  A: { liq: 7000, ltv: 3000, minMargin: 1200 },
  B: { liq: 6500, ltv: 3000, minMargin: 1200 },
  C: { liq: 5000, ltv: 2500, minMargin: 1000 },
  D: { liq: 4000, ltv: 2000, minMargin: 800 },
} as const;

export const TIER_INDEX: Record<TierName, number> = { A: 0, B: 1, C: 2, D: 3 };

/** depositCapStock in base units (8 decimals); basisCapUsdc in USDC base units (6 decimals). */
export function vaultParams(tier: TierName, depositCapStock: bigint, basisCapUsdc: bigint) {
  const t = TIER[tier];
  return {
    ltvBps: t.ltv,
    minMarginBps: t.minMargin,
    liqLtvBps: t.liq,
    emergencyLtvBps: t.liq - 500,
    enterMarginBps: 200,
    exitMarginBps: 100,
    carryGuardMarginBps: 50,
    minEnterFundingBps: 450,
    expectedHoldHours: 720,
    roundtripCostBps: 60,
    fundingWindow: 24,
    maxSwapSlippageBps: 50,
    maxPerpSlippageBps: 30,
    maxIndexDevBps: 50,
    hedgeTolBps: 50,
    sizeBandBps: 300,
    rebalanceLtvBandBps: 800,
    rebalanceMarginBandBps: 500,
    maxNavAgeSlots: new BN(150),
    epochLenSecs: new BN(3600),
    perfFeeBps: 1500,
    exitFeeBps: 10,
    depositCapStock: new BN(depositCapStock.toString()),
    basisCapUsdc: new BN(basisCapUsdc.toString()),
  };
}
