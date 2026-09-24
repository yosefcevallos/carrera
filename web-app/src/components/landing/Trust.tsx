export default function Trust() {
  return (
    <section className="trust" id="safety" aria-labelledby="trust-t">
      <div className="wrap">
        <h2 className="h2" id="trust-t">
          How your stock is looked after
        </h2>
        <div className="tgrid">
          <div>
            <h3>You keep the price moves</h3>
            <p>If your stock rises 20%, your deposit rises 20%. The extra trade is hedged, so it doesn&apos;t add or remove exposure.</p>
          </div>
          <div>
            <h3>It never chases a bad rate</h3>
            <p>When funding is low, the vault closes the trade and parks its USDC on Kamino, or repays the loan if that would lose money, switching back when funding recovers.</p>
          </div>
          <div>
            <h3>Only you can withdraw</h3>
            <p>Everything is held by the on-chain program. The team and the bots that run it can never move your funds.</p>
          </div>
          <div>
            <h3>Fees only on earnings</h3>
            <p>15% of the USDC you earn. Never a cut of your stock or its gains.</p>
          </div>
        </div>
        <p className="risk">
          Earnings aren&apos;t guaranteed. The trade uses borrowing, and an extremely sharp move in a stock can still cause losses despite automatic rebalancing every minute.
        </p>
      </div>
    </section>
  );
}
