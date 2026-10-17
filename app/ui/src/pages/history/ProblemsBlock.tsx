import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { GameProblems } from "@/shared/api/bindings/GameProblems";
import type { Problem } from "@/shared/api/bindings/Problem";
import { api, errorText } from "@/shared/api/commands";
import { Chip } from "@/shared/ui/Chip";
import styles from "@/shared/ui/lab.module.css";

const SHOWN = 30;

export function ProblemsBlock({ dir, game }: { dir: string; game: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [data, setData] = useState<GameProblems | null>(null);
  const [error, setError] = useState<string | null>(null);
  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (next && data === null) {
      api
        .gameProblems(dir, game)
        .then(setData)
        .catch((reason: unknown) => setError(errorText(reason)));
    }
  };
  const fresh = new Set(data?.new.map(problemKey) ?? []);
  const sorted = [...(data?.problems ?? [])].sort(
    (a, b) =>
      Number(fresh.has(problemKey(b))) - Number(fresh.has(problemKey(a))) ||
      Number(b.severity === "error") - Number(a.severity === "error") ||
      b.count - a.count,
  );
  return (
    <div className={styles.problems}>
      <button type="button" className={styles.linkButton} onClick={toggle}>
        {open ? t("problems.hide") : t("problems.show")}
      </button>
      {open && error ? <span className={styles.muted}>{error}</span> : null}
      {open && data ? (
        <>
          <span className={styles.muted}>
            {data.comparedWith
              ? t("problems.newSince", { count: data.new.length, run: data.comparedWith })
              : t("problems.noEarlier")}
          </span>
          <ul className={styles.problemList}>
            {sorted.slice(0, SHOWN).map((problem, index) => (
              <li key={index} className={fresh.has(problemKey(problem)) && data.comparedWith ? styles.fresh : undefined}>
                <Chip tone={problem.severity === "error" ? "bad" : "warn"}>{problem.count}</Chip>
                <span className={styles.problemText} title={problem.text}>
                  <b>{problem.source}</b> {problem.text}
                </span>
              </li>
            ))}
          </ul>
          {sorted.length > SHOWN ? <span className={styles.muted}>{t("problems.more", { count: sorted.length - SHOWN })}</span> : null}
        </>
      ) : null}
    </div>
  );
}

function problemKey(problem: Problem): string {
  return `${problem.source}|${problem.severity}|${problem.text}`;
}
