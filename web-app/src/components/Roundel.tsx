export default function Roundel({ n, className = "" }: { n: number; className?: string }) {
  return <span className={`roundel ${className}`.trim()}>{n}</span>;
}
