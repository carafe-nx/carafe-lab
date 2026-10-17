import type { ReactNode } from "react";
import styles from "./Chip.module.css";

type ChipProps = {
  tone?: "ok" | "warn" | "bad" | "acc" | "plain";
  children: ReactNode;
};

export function Chip({ tone = "plain", children }: ChipProps) {
  return <span className={`${styles.chip} ${styles[tone]}`}>{children}</span>;
}
