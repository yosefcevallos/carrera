"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";

export default function LightsOut() {
  const [lit, setLit] = useState(0);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const ref = useRef<HTMLDivElement>(null);

  const clear = () => {
    timers.current.forEach(clearTimeout);
    timers.current = [];
  };
  const run = () => {
    clear();
    for (let i = 1; i <= 5; i++) timers.current.push(setTimeout(() => setLit(i), (i - 1) * 170));
  };
  const reset = () => {
    clear();
    setLit(0);
  };

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (es) =>
        es.forEach((e) => {
          if (e.isIntersecting) {
            run();
            timers.current.push(setTimeout(reset, 1800));
          }
        }),
      { threshold: 0.6 },
    );
    io.observe(el);
    return () => {
      io.disconnect();
      clear();
    };
  }, []);

  return (
    <section className="cta" aria-labelledby="cta-t">
      <div className="wrap">
        <div>
          <div className="lights" ref={ref} aria-hidden="true">
            {[1, 2, 3, 4, 5].map((i) => (
              <i key={i} className={lit >= i ? "on" : ""} />
            ))}
          </div>
          <h2 id="cta-t">LIGHTS OUT</h2>
        </div>
        <div className="right">
          <p className="serif">Put your stocks on the grid.</p>
          <Link className="btn w" href="/app" onMouseEnter={run} onFocus={run} onMouseLeave={reset} onBlur={reset}>
            Launch app
          </Link>
        </div>
      </div>
    </section>
  );
}
