import { useTranslation } from "react-i18next";
import type { Outcome } from "@/shared/api/bindings/Outcome";
import { outcomeTone } from "./outcome";
import styles from "./RecordDots.module.css";

export function RecordDots({ outcomes }: { outcomes: Outcome[] }) {
  const { t } = useTranslation();
  if (outcomes.length === 0) {
    return null;
  }
  const failed = outcomes.filter((outcome) => outcome === "fail").length;
  const summary =
    failed === 0
      ? t("record.clean", { count: outcomes.length })
      : t("record.failed", { failed, total: outcomes.length });
  return (
    <span className={styles.record} title={summary}>
      <span className={styles.dots} aria-hidden="true">
        {[...outcomes].reverse().map((outcome, index) => (
          <i key={index} className={styles[outcomeTone(outcome)]} />
        ))}
      </span>
      <span className={failed > 0 && failed < outcomes.length ? styles.flaky : undefined}>{summary}</span>
    </span>
  );
}
