import { VAULT_META, type Ticker } from "@/constants/vaults";

/** Official xStock token icons live in public/icons/{TICKER}.png (square). */
export const tokenIconSrc = (t: Ticker) => `/icons/${t}.png`;

/**
 * Circular token icon with a 2px ink ring so it reads on bone and on the red header band.
 * The vault's roundel number is kept as the alt text only.
 */
export default function TokenIcon({ t, size = 34, className = "", eager = false }: { t: Ticker; size?: number; className?: string; eager?: boolean }) {
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      className={`ticon ${className}`.trim()}
      src={tokenIconSrc(t)}
      alt={String(VAULT_META[t].roundel)}
      width={size}
      height={size}
      loading={eager ? "eager" : "lazy"}
      decoding="async"
    />
  );
}
