/** Small bar waveform. Values are relative; the tallest bar fills the height. */
export default function Wave({ values, className = "" }: { values: number[]; className?: string }) {
  const max = Math.max(1e-9, ...values.map((v) => Math.abs(v)));
  return (
    <span className={`wave ${className}`.trim()} aria-hidden="true">
      {values.map((v, i) => (
        <i key={i} style={{ height: `${Math.round((Math.max(0, v) / max) * 100)}%` }} />
      ))}
    </span>
  );
}
