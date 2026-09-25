import { VAULT_META, type Ticker } from "@/constants/vaults";

/** Official xStock token icons live in public/icons/{TICKER}.png (square, with dark corner wedges). */
export const tokenIconSrc = (t: Ticker) => `/icons/${t}.png`;

/**
 * Circular token icon with a 2px ink ring so it reads on bone and on the red header band.
 * The image is scaled 1.15× inside the circular clip so the PNG's corner wedges never show.
 * The vault's roundel number is kept as the alt text only.
 */
export default function TokenIcon({ t, size = 34, className = "", eager = false }: { t: Ticker; size?: number; className?: string; eager?: boolean }) {
  return (
    <span className={`ticon ${className}`.trim()} style={{ width: size, height: size }}>
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img src={tokenIconSrc(t)} alt={String(VAULT_META[t].roundel)} width={size} height={size} loading={eager ? "eager" : "lazy"} decoding="async" />
    </span>
  );
}
