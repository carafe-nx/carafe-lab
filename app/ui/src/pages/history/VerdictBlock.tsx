import { useTranslation } from "react-i18next";
import type { VariantResult } from "@/shared/api/bindings/VariantResult";
import type { Verdict } from "@/shared/api/bindings/Verdict";
import { Chip } from "@/shared/ui/Chip";
import styles from "@/shared/ui/lab.module.css";
import history from "./HistoryPage.module.css";

export function VerdictBlock({ verdict }: { verdict: Verdict }) {
  const { t } = useTranslation();
  const percent = verdict.differencePercent === null ? null : Math.round(Math.abs(verdict.differencePercent));
  const slower = (verdict.differencePercent ?? 0) > 0;
  const fpsPercent = verdict.fpsDifferencePercent === null ? null : Math.round(Math.abs(verdict.fpsDifferencePercent));
  const fpsHigher = (verdict.fpsDifferencePercent ?? 0) > 0;
  return (
    <div className={history.verdict}>
      <span className={styles.label}>{t("ab.verdict")}</span>
      <Variant name="A" result={verdict.a} />
      <Variant name="B" result={verdict.b} />
      {percent === null ? null : verdict.significant ? (
        <Chip tone={slower ? "bad" : "ok"}>{t(slower ? "ab.slower" : "ab.faster", { percent })}</Chip>
      ) : (
        <span className={styles.muted}>{t("ab.noise")}</span>
      )}
      {fpsPercent === null ? null : verdict.fpsSignificant ? (
        <Chip tone={fpsHigher ? "ok" : "bad"}>{t(fpsHigher ? "ab.fpsHigher" : "ab.fpsLower", { percent: fpsPercent })}</Chip>
      ) : (
        <span className={styles.muted}>{t("ab.fpsNoise")}</span>
      )}
    </div>
  );
}

function Variant({ name, result }: { name: string; result: VariantResult }) {
  const { t } = useTranslation();
  return (
    <div className={history.variant}>
      <b>{name}</b>
      <span>{t("ab.runs", { passed: result.passed, runs: result.runs })}</span>
      <span className={styles.muted}>
        {result.medianFirstFrame === null
          ? t("ab.noMedian")
          : t("ab.median", { seconds: Math.round(result.medianFirstFrame), spread: Math.round(result.spread ?? 0) })}
        {result.medianFps === null ? "" : ` · ${t("ab.fps", { fps: Math.round(result.medianFps) })}`}
      </span>
    </div>
  );
}
