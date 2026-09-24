# Carrera — Frontend handoff

Two surfaces, one visual system:

| Surface | URL | Purpose |
|---|---|---|
| Landing | `carrera.xyz` | Explain the product in plain words and send people to the app |
| App | `app.carrera.xyz` | Browse the nine vaults, deposit xStocks, withdraw stock plus USDC |

The chosen app layout is **Prototype B (rows)**: a vault table, and a wide vault window with a share price chart on the left and the deposit / withdraw form on the right.

## What's in this folder

| File | What it is |
|---|---|
| `carrera-reference.html` | The clickable reference. Open it in a browser. `#/` is the landing page, `#/app` is the app. All data is mocked. |
| `HANDOFF.md` | This spec |
| `tokens.css` | Colors, type and spacing as CSS variables, light and dark |
| `vaults.sample.json` | The shape of the data the UI needs, with the demo values |

The reference is a single file for convenience. Treat it as the source of truth for **look, copy and behavior**, not as production code. Rebuild it as components in your stack (suggested below).

---

## 1. Visual direction

1950s–70s Italian grand prix heritage, done typographically rather than with photography:

- Bone paper background, ink type and **rosso corsa** red as the only strong color
- A giant cropped wordmark, tightly tracked, in Archivo at its widest setting
- White number roundels, one per vault (1–9), used everywhere a vault appears
- A small green-white-black tricolore mark next to the wordmark (only there)
- Instrument Serif italic as the "Italian voice": greetings, company names, the hero line
- Square corners everywhere (no border radius), except circles (roundels, bubbles, status dots)
- Hairline dividers and 2px ink rules instead of cards and shadows

Red is reserved for things that are live or yours: the top bar, yields earning from funding, the selected bubble, rows you hold, focus rings.

## 2. Design tokens

See `tokens.css`. Summary:

| Token | Light | Dark | Use |
|---|---|---|---|
| `--bone` | `#DDDAD0` | `#161614` | Page background |
| `--paper` | `#E9E6DD` | `#201F1C` | Panels, calculator, highlighted rows, chart background |
| `--ink` | `#161513` | `#ECE8DE` | Text, rules, filled bubbles, primary buttons |
| `--grey` | `#6A665C` | `#A39F93` | Secondary text |
| `--line` | `#B8B3A5` | `#3B3934` | Hairlines, dashed lane lines |
| `--corsa` | `#E3241D` | same | Top bar, live and yours, focus |
| `--paused` | `#5A5750` | `#45433E` | Header of a vault that's lending |
| black | `#000000` | same | "Three laps" section, footer, toasts |

Dark mode follows `prefers-color-scheme`, with `data-theme="light|dark"` on `<html>` as an override.

## 3. Typography

Fonts (Google Fonts):

- **Archivo** variable, axes `wdth 62–125` and `wght 100–900`. One family for display, UI and numbers.
- **Instrument Serif**, italic.

| Role | Spec |
|---|---|
| Giant wordmark | Archivo 900, `wdth 125`, `clamp(96px, 21.2vw, 440px)`, line-height .74, letter-spacing −.025em, cropped under the bar (`margin-top: −.13em`) |
| Section display ("THREE LAPS", "LIGHTS OUT") | Archivo 900, `wdth 125`, uppercase, line-height .82–.84 |
| H2 | Archivo 800, `wdth 110`, `clamp(36px, 4.2vw, 56px)`, line-height 1 |
| Tickers | Archivo 900, `wdth 125`, uppercase (they are tickers) |
| Big numbers | Archivo 800, `wdth 95`, tabular figures |
| Body | Archivo 400, 16–17px, line-height 1.5 |
| Small labels | Archivo 400–600, 12–13px, `--grey`, sentence case |
| Serif voice | Instrument Serif italic, 22–44px |

Sentence case everywhere except tickers and the two uppercase display words. Numbers use `font-variant-numeric: tabular-nums`.

## 4. Layout and breakpoints

- Content max width 1320px, side padding 40px (20px on mobile)
- Breakpoints: **1000px** (grids go to two columns, the modal chart stacks at 860px) and **680px** (single column, top-bar nav hides)
- The red bar is sticky, 40px tall, three columns: nav left, wordmark centered, action right
- Respect safe areas: `viewport-fit=cover` plus `env(safe-area-inset-*)` padding

---

## 5. Landing page (`carrera.xyz`)

### 5.1 Top bar
Left: "How it works", "Earnings", "Safety" (smooth-scroll to sections). Center: tricolore mark plus italic "Carrera" wordmark. Right: black "Launch app" chip, which goes to `app.carrera.xyz`.

### 5.2 Hero
1. **Wordmark** "CARRERA", cropped by the bar and bleeding off both sides. On load it animates `wdth 62 → 125` over 1.1s (ease `cubic-bezier(.2,.8,.1,1)`). This is the page's one orchestrated motion. Skip it under `prefers-reduced-motion`.
2. **Lede**: serif line "Your stocks, with a second engine." plus sans sub line "Deposit the tokenized stocks you already own. Keep every gain, and earn extra in USDC while you hold."
3. **Racing bubbles**, three horizontal lanes separated by dashed lines:
   - Lanes: `[TSLA, NVDA, AAPL]`, `[SPY, GOOGL, HOOD]`, `[MSTR, QQQ, CRCL]`
   - Each lane scrolls left to right forever, at 46s, 34s and 58s per loop. Content is duplicated and translated from −50% to 0 for a seamless loop.
   - **Diameter = vault size**: `56 + sqrt(tvl / maxTvl) × 72` px (roughly 70–128px)
   - Filled ink means earning from funding. Paper with an ink outline means earning from lending. Selected is red with a 4px bone gap and a 2px red ring.
   - Inside: ticker (the logo once available) and yield, for example "+8.2%"
   - Three short trailing "speed lines" to the left of each bubble
   - Hovering or focusing inside the lanes pauses all lanes. Clicking selects that stock.
   - Reduced motion: lanes don't move
   - Key below: "Earning from funding", "Earning from lending", "Bubble size shows vault size"
4. **Caption** for the selected stock (default TSLA): ticker plus company, one sentence, and CTAs "Deposit TSLAx" (deep-links to the app with that vault open) and "See how it works".
   - Funding copy: *"Hold {Company} and earn about {y}% a year on top, paid in USDC. If {Company} goes up, every bit of that is still yours."*
   - Lending copy: *"Funding on {Company} is low right now, so this vault lends its USDC on Kamino and earns about {y}% a year. It switches back to funding on its own when rates pick up."*

Faint concentric rings sit behind the hero (decorative, `--line`).

### 5.3 Three laps (black section)
"THREE LAPS", sub "That's the whole race. No trading, no charts to watch." Three cards, the middle one red:
1. **Deposit your stock**, with a number-plate illustration
2. **It earns while you hold**, with a flat race car illustration
3. **Take it back, plus USDC**, with a checkered flag illustration

The illustrations are inline SVG in the reference. Keep them flat, with no gradients.

### 5.4 Calculator ("What could your stocks earn?")
- Stock chips (9), a dollar amount slider ($500–$50,000, step $500)
- Output: "You keep 100% of {Company}'s price moves" and "You'd earn, est. ${amount × yield} a year"
- Fine print: *"Before Carrera's 15% fee on earnings. Not a promise: rates move every hour, and when funding is low a vault earns the lower Kamino lending rate instead."*

### 5.5 How your stock is looked after
Four columns: You keep the price moves, It never sits idle, Only you can withdraw, Fees only on earnings. Then a visible risk sentence. Keep that sentence. It's there on purpose.

### 5.6 Lights out CTA and footer
Red band: five start lights that light up one by one (170ms apart) when the section scrolls into view or the button is hovered, then "LIGHTS OUT", "Put your stocks on the grid." and a white "Launch app" button. Black footer with the disclaimer.

---

## 6. App (`app.carrera.xyz`)

### 6.1 Top bar
Left: "Vaults", "About Carrera" (goes to the landing page). Center: wordmark. Right: wallet chip ("Connect wallet", or the truncated address with a red dot once connected). Clicking a connected chip disconnects.

### 6.2 Protocol stats (always visible)
A five-column strip with a 2px ink top rule:

| Stat | Example | Note |
|---|---|---|
| Total value locked | $6.81M | Sum of all vaults, USD |
| Average yield | 5.1% | TVL-weighted current yield, USDC, shown red |
| Earning from funding | 6 | "of 9 vaults, the rest are lending" |
| USDC paid out, 24h | $1,062 | |
| Depositors | 1,284 | Unique depositors |

### 6.3 Your strip (only when connected)
Paper band with a red left edge: greeting ("Buongiorno." before 17:00, "Buonasera." after), your deposits in USD with the list of positions, USDC earned so far ("paid when you withdraw"), and your earning rate (value-weighted).

### 6.4 Vault table
Heading "Choose a stock to put to work", sub "Every stock has its own vault. Rates update every hour." Filters: **All**, **Funding**, **Yours**.

| Column | Content |
|---|---|
| Vault | Roundel (1–9), ticker, company in serif italic |
| Status | Red dot "Funding", or outlined dot "Lending" |
| Earns | Yield, a year, in USDC. Red when funding, ink when lending. |
| Last 24h | Small bar waveform of hourly funding |
| Your deposit | "10.00 TSLAx" plus "+$12.98 earned", or "—". While withdrawing: "Withdrawal settling…" or "Ready to claim". |
| Action | "Deposit", "Manage", or "Claim" |

Rows you hold (or have a pending withdrawal in) get a paper background and a 5px red left edge. The whole row opens the vault window. Rows are focusable, and Enter or Space opens them.

Mobile: Status and Last 24h move under the ticker, and Your deposit and Action hide (tap the row).

Empty "Yours": *"You haven't deposited yet. Choose All to see every vault."*, or *"Connect a wallet to see your vaults."*

### 6.5 Vault window (modal)
Width up to 1060px, top-aligned, dark scrim. Closes with the × button, Escape, or a click on the scrim. Focus moves into it on open and returns to the row on close.

**Header** (red when funding, `--paused` grey when lending): roundel, status ("Earning from funding" or "Earning from lending"), ×, big ticker, company in serif.

**Left: share price chart**
- Title "Share price" with value **"1 TSLAx + 7.794 USDC"**. Each share is one xStock plus the USDC it has earned, and the USDC part only goes up.
- Sub line: "Each share is one TSLAx plus the USDC it has earned. +1.940 USDC in 30 days."
- Range buttons 7D / 30D / 90D (default 30D)
- Area line chart of USDC per share, with a crosshair and tooltip on hover or touch (date, value, mode)
- A mode strip under the chart, one segment per day: red for funding, hatched for lending
- Legend, then three stats: Earning now, Mode, Vault size
- Stacks above the form under 860px

**Right: form.** Tabs "Deposit" and "Withdraw". The default tab is Withdraw if you hold a position, otherwise Deposit.

*Deposit*
- Amount input in the xStock (for example "5.00 NVDAx"), Max button, wallet balance and ≈ USD value
- If lending, a note: *"Earning from lending for now. Funding on {Company} is low, so the vault lends its USDC on Kamino. It switches back to funding on its own when rates pick up."*
- Two figures: "You keep 100% of {Company}'s price moves" and "You earn, est. ${value × yield} a year"
- Button "Deposit {T}x" ("Connect wallet to deposit" when disconnected)
- Validation: empty gives *"Enter how much to deposit."*; too much gives *"That's more than the {bal} {T}x in your wallet."*
- Success toast: *"Deposited {n} {T}x. You keep every move in {Company}'s price."* The window then switches to Withdraw.

*Withdraw*
- Amount in the xStock, "All" button, deposited amount and USDC earned
- "You get back {n} {T}x" and "Plus {usdc} USDC, after the 15% fee"
- Note: *"Ready within the hour. Withdrawals settle at the top of each hour. Your stock keeps earning until then."*
- After submitting, a three-step tracker: Requested, Settling at the top of the hour, Ready to claim. When ready: "Ready to claim {n} {T}x plus {usdc} USDC" and a red "Claim to wallet" button.
- Nothing deposited: *"Nothing to withdraw yet"* plus a "Deposit {T}x" button

*How this vault works* (collapsed by default): what it does, what happens when funding is low, borrowed against deposits (LTV), protected until a stock move of (liquidation buffers), rebalanced every minute, fees.

### 6.6 Toasts
Black, 4px red left edge, bottom center, about 3.6s, `role="status"`. Past tense, and they name what happened.

---

## 7. Data the UI needs

See `vaults.sample.json` for the exact shape.

**Per vault**

| Field | Meaning | Likely source |
|---|---|---|
| `ticker`, `name`, `roundel` | Display | Static config |
| `mode` | `"funding"` or `"lending"` | Vault program state |
| `yieldApy` | Current estimated USDC yield on stock value, % | Derived: `LTV × fundingAPY − LTV(1+LTV) × borrowAPY − costs`, or the lending yield in lending mode |
| `ltv` | Tier LTV (20 / 25 / 30%) | Vault config |
| `liqBuffer` | `{ down, up }` % stock move before liquidation | Computed from Kamino and Phoenix positions |
| `tvlUsd` | Vault size | Vault accounts × oracle price |
| `funding24h[]` | Hourly funding for the waveform | Phoenix / Hawkeye |
| `sharePrice` | `{ stockPerShare, usdcPerShare }` | Vault accounts |
| `sharePriceHistory[]` | Daily `{ date, usdcPerShare, mode }` for 90 days | Indexer |
| `capacityUsd` | Max vault size (Phoenix open interest limits) | Config or computed |

**Per user**: xStock wallet balances, per-vault `{ shares, stockAmount, usdcEarned }`, and pending withdrawals `{ amount, usdcAmount, readyAt, ready }`.

**Protocol**: TVL, TVL-weighted yield, count in funding mode, USDC paid out in the last 24h, depositors.

Refresh vault data every few minutes. Rates change hourly, so hourly is the minimum.

## 8. Suggested stack

- Next.js (two apps, or one app with host-based routing for `carrera.xyz` and `app.carrera.xyz`)
- `@solana/wallet-adapter` (Phantom, Solflare, Backpack)
- React Query for polling vault and protocol data
- Chart: custom SVG (as in the reference) or visx / Recharts, styled to match. No gridline clutter: three hairlines, no axes except the start date and "Today".
- CSS: tokens from `tokens.css`. Tailwind or CSS Modules both work.

## 9. Accessibility and motion

- Visible focus: 2px red outline, 3px offset
- The bubble lanes pause on hover and focus, and stop entirely under `prefers-reduced-motion`
- Only the first copy of each bubble is focusable (the duplicates for the loop are `aria-hidden`)
- The modal uses `role="dialog"`, `aria-modal`, Escape to close, and returns focus
- Toasts and form errors are announced (`role="status"`, `role="alert"`)
- Minimum body text 12px, contrast checked against bone and paper

## 10. Assets

- **Stock logos**: not included. They're trademarks, so source the official xStocks token icons from Backed, or each company's press kit, and follow their usage rules. The reference has a `LOGOS` map that swaps the ticker text for an image inside each bubble.
- **Illustrations**: the three flat SVGs in "Three laps" are original and can be reused as they are.
- **Fonts**: Archivo and Instrument Serif are both on Google Fonts under the OFL.

## 11. Open questions before launch

1. **Lending mode economics.** The UI shows the gross Kamino lending rate for vaults in lending mode. With borrow above supply, keeping the loan and lending it is slightly negative net of borrow cost. Confirm whether lending mode keeps the loan (and which number to show), or repays it and shows 0%.
2. **Thresholds.** "Switches to lending when funding falls below 6%" is a placeholder. Supply the real entry and exit thresholds.
3. **Fees display.** Yields shown are before the 15% performance fee. Decide whether cards and rows should show net.
4. **Prices, yields, TVL, depositors** in the reference are demo numbers.
5. **Capacity.** AAPL's Phoenix open interest is tiny. Decide whether to show capacity per vault, or block deposits when full.
