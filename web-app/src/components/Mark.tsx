import Link from "next/link";

export default function Mark() {
  return (
    <Link className="mark" href="/" aria-label="Carrera home">
      <span className="tri" aria-hidden="true">
        <i />
        <i />
        <i />
      </span>
      Carrera
    </Link>
  );
}
