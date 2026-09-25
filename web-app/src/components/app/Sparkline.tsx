import { sparkPath } from "@/lib/app2";

/** 7-day funding line on a scale shared across the table (`max`), dashed zero line, dot on the last point. */
export default function Sparkline({ series, max, funding, w = 110, h = 24 }: { series: number[]; max: number; funding: boolean; w?: number; h?: number }) {
  const g = sparkPath(series, max, w, h);
  const cls = funding ? "spark on" : "spark";
  return (
    <svg className={cls} width={w} height={h} viewBox={`0 0 ${w} ${h}`} aria-hidden="true">
      <line x1="0" x2={w} y1={g.midY} y2={g.midY} className="zero" />
      {g.path && <path d={g.path} />}
      {g.last && <circle cx={g.last.x} cy={g.last.y} r="2.2" />}
    </svg>
  );
}
