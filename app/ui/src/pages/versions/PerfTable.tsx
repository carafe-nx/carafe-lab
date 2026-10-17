import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Spread } from "@/shared/api/bindings/Spread";
import type { Trend } from "@/shared/api/bindings/Trend";
import type { VersionStep } from "@/shared/api/bindings/VersionStep";
import { api, errorText } from "@/shared/api/commands";
import { Notice } from "@/shared/ui/Notice";
import styles from "@/shared/ui/lab.module.css";
import versions from "./VersionsPage.module.css";

type Measure = { key: string; spread: Spread | null; trend: Trend | null; format: (value: number) => string };

export function PerfTable({ refreshKey }: { refreshKey: string }) {
  const { t } = useTranslation();
  const [steps, setSteps] = useState<VersionStep[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api
      .versionSteps()
      .then((list) => {
        setSteps(list);
        setError(null);
      })
      .catch((reason: unknown) => setError(errorText(reason)));
  }, [refreshKey]);
  const commits = [...new Set(steps.map((step) => step.perf.commit))];
  const games = [...new Set(steps.map((step) => step.perf.game))];
  const cell = (game: string, commit: string) =>
    steps.find((step) => step.perf.game === game && step.perf.commit === commit);
  if (error) return <Notice tone="error">{error}</Notice>;
  if (steps.length === 0) return null;
  return (
    <>
      <h2 className={versions.heading}>{t("versions.perfTitle")}</h2>
      <p className={styles.muted}>{t("versions.perfHint")}</p>
      <div className={versions.scroll}>
        <table className={versions.table}>
          <thead>
            <tr>
              <th>{t("versions.game")}</th>
              {commits.map((commit) => (
                <th key={commit} className={versions.commit}>
                  {commit}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {games.map((game) => (
              <tr key={game}>
                <th>{game}</th>
                {commits.map((commit) => {
                  const step = cell(game, commit);
                  if (!step) {
                    return (
                      <td key={commit}>
                        <span className={styles.muted}>—</span>
                      </td>
                    );
                  }
                  const measures: Measure[] = [
                    { key: "firstFrame", spread: step.perf.firstFrame, trend: step.firstFrame, format: (value) => `${Math.round(value)} ${t("versions.sec")}` },
                    { key: "fps", spread: step.perf.fps, trend: step.fps, format: (value) => `${Math.round(value)}` },
                    { key: "p99", spread: step.perf.p99Ms, trend: step.p99Ms, format: (value) => `${Math.round(value)} ${t("versions.ms")}` },
                    { key: "memory", spread: step.perf.peakMb, trend: step.peakMb, format: (value) => `${(value / 1024).toFixed(1)} ${t("versions.gb")}` },
                  ];
                  const repeats = Math.max(...measures.map((measure) => measure.spread?.count ?? 0));
                  return (
                    <td key={commit}>
                      <div className={versions.measures}>
                        {measures
                          .filter((measure) => measure.spread !== null)
                          .map((measure) => (
                            <span key={measure.key} className={trendClass(measure.trend)}>
                              {t(`versions.measure.${measure.key}`)} {measure.format(measure.spread!.median)}
                              {measure.spread!.count > 1 ? ` ±${measure.format(measure.spread!.spread / 2)}` : ""}
                              {trendMark(measure.trend)}
                            </span>
                          ))}
                        <span className={styles.muted}>{t("versions.repeats", { count: repeats })}</span>
                      </div>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

function trendClass(trend: Trend | null): string | undefined {
  if (trend === "better") return versions.better;
  if (trend === "worse") return versions.worse;
  return undefined;
}

function trendMark(trend: Trend | null): string {
  if (trend === "better") return " ▲";
  if (trend === "worse") return " ▼";
  return "";
}
