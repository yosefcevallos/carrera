import Roundel from "@/components/Roundel";

export default function Laps() {
  return (
    <section className="laps" id="how" aria-labelledby="laps-t">
      <div className="wrap">
        <h2 id="laps-t">HOW IT WORKS</h2>
        <p className="sub">That&apos;s the whole race. No trading, no charts to watch.</p>
        <div className="lapgrid">
          <article className="lap">
            <div className="n">
              <span>Lap 1</span>
              <Roundel n={1} />
            </div>
            <svg viewBox="0 0 300 120" aria-hidden="true">
              <rect x="70" y="14" width="160" height="92" fill="#2F2E2B" />
              <rect x="70" y="14" width="10" height="92" fill="#0F7B45" />
              <rect x="80" y="14" width="10" height="92" fill="#fff" />
              <rect x="90" y="14" width="10" height="92" fill="#000" />
              <text x="165" y="76" textAnchor="middle" fontFamily="Archivo,Arial" fontWeight="900" fontSize="40" fill="#EAE6DC">
                TSLA
              </text>
            </svg>
            <h3>Deposit your stock</h3>
            <p>Bring the xStock you already own, like TSLAx or SPYx. It stays yours, and so does every move in its price.</p>
          </article>
          <article className="lap">
            <div className="n">
              <span>Lap 2</span>
              <Roundel n={2} />
            </div>
            <svg viewBox="0 0 300 120" aria-hidden="true">
              <path d="M30 78 C30 60 70 54 120 53 L220 52 C252 52 270 62 270 74 C270 84 262 88 250 88 L44 90 C36 90 30 86 30 78Z" fill="#fff" />
              <path d="M138 52 C146 38 162 34 176 40 L184 52Z" fill="#000" />
              <circle cx="160" cy="38" r="9" fill="#000" />
              <circle cx="108" cy="72" r="13" fill="#E3241D" stroke="#000" strokeWidth="2" />
              <text x="108" y="77" textAnchor="middle" fontFamily="Archivo,Arial" fontWeight="900" fontSize="14" fill="#fff">
                5
              </text>
              <circle cx="74" cy="92" r="17" fill="#000" />
              <circle cx="234" cy="92" r="18" fill="#000" />
              <path d="M10 64H40M0 76H28M14 88H36" stroke="#fff" strokeWidth="3" opacity=".6" />
            </svg>
            <h3>It earns while you hold</h3>
            <p>Carrera borrows a small slice against your stock and runs a hedged trade that collects fees from traders. Nothing for you to manage.</p>
          </article>
          <article className="lap">
            <div className="n">
              <span>Lap 3</span>
              <Roundel n={3} />
            </div>
            <svg viewBox="0 0 300 120" aria-hidden="true">
              <defs>
                <pattern id="chk" width="14" height="14" patternUnits="userSpaceOnUse">
                  <rect width="14" height="14" fill="#EAE6DC" />
                  <rect width="7" height="7" fill="#161513" />
                  <rect x="7" y="7" width="7" height="7" fill="#161513" />
                </pattern>
              </defs>
              <path d="M96 10 L96 116" stroke="#EAE6DC" strokeWidth="4" />
              <path d="M98 14 C130 4 160 30 190 20 C212 12 228 22 238 30 L232 82 C218 72 204 66 186 76 C158 90 130 66 100 76Z" fill="url(#chk)" />
            </svg>
            <h3>Take it back, plus USDC</h3>
            <p>Withdraw whenever you like. You get your stock back plus the USDC it earned, usually within the hour.</p>
          </article>
        </div>
      </div>
    </section>
  );
}
