"use client";

import { useEffect, useState } from "react";
import { useUiStore } from "@/store/ui-provider";

export default function Toast() {
  const toast = useUiStore((s) => s.toast);
  const seq = useUiStore((s) => s.toastSeq);
  const [show, setShow] = useState(false);

  useEffect(() => {
    if (!seq) return;
    setShow(true);
    const t = setTimeout(() => setShow(false), 3600);
    return () => clearTimeout(t);
  }, [seq]);

  return (
    <div className={`toast${show ? " show" : ""}`} role="status">
      {toast}
    </div>
  );
}
