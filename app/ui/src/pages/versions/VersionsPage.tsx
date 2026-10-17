import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { outcomeTone } from "@/features/lab/outcome";
import type { Outcome } from "@/shared/api/bindings/Outcome";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { Chip } from "@/shared/ui/Chip";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";
import versions from "./VersionsPage.module.css";

type Cell = { outcome: Outcome; runs: number; failed: number; dir: string; fps: number | null };

type Matrix = { commits: string[]; games: string[]; cells: Map<string, Cell> };

function baseName(name: string): string {
  return name.replace(/ · [AB]$/, "");
}

function buildMatrix(runs: RunSummary[]): Matrix {
  const oldestFirst = [...runs].reverse();
  const commits: string[] = [];
  const games: string[] = [];
  const cells = new Map<string, Cell>();
  for (const run of oldestFirst) {
    const commit = run.stamp.carafeCommit;
    if (!commit) continue;
    if (!commits.includes(commit)) commits.push(commit);
    for (const game of run.games) {
      if (game.outcome === "skip") continue;
      const name = baseName(game.name);
      if (!games.includes(name)) games.push(name);
      const key = `${name}\u0000${commit}`;
      const known = cells.get(key);
      cells.set(key, {
        outcome: game.outcome,
        runs: (known?.runs ?? 0) + 1,
        failed: (known?.failed ?? 0) + (game.outcome === "fail" ? 1 : 0),
        dir: run.dir,
        fps: game.perf?.medianFps ?? known?.fps ?? null,
      });
    }
  }
  return { commits, games, cells };
}

export function VersionsPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { runs } = useLab();
  const matrix = buildMatrix(runs.data ?? []);
  return (
    <Page title={t("versions.title")}>
      <p className={styles.muted}>{t("versions.hint")}</p>
      {matrix.commits.length === 0 ? <Notice tone="info">{t("versions.empty")}</Notice> : null}
      {matrix.commits.length > 0 ? (
        <div className={versions.scroll}>
          <table className={versions.table}>
            <thead>
              <tr>
                <th>{t("versions.game")}</th>
                {matrix.commits.map((commit) => (
                  <th key={commit} className={versions.commit}>
                    {commit}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {matrix.games.map((game) => (
                <tr key={game}>
                  <th>{game}</th>
                  {matrix.commits.map((commit, index) => {
                    const cell = matrix.cells.get(`${game}\u0000${commit}`);
                    const before = index > 0 ? matrix.cells.get(`${game}\u0000${matrix.commits[index - 1]}`) : undefined;
                    const broke = cell?.outcome === "fail" && before !== undefined && before.outcome !== "fail";
                    return (
                      <td key={commit} className={broke ? versions.broke : undefined}>
                        {cell ? (
                          <button type="button" className={versions.cell} onClick={() => navigate(`/history/${cell.dir}`)}>
                            <Chip tone={outcomeTone(cell.outcome)}>{t(`outcome.${cell.outcome}`)}</Chip>
                            <span className={styles.muted}>
                              {cell.failed > 0 ? t("record.failed", { failed: cell.failed, total: cell.runs }) : t("versions.runs", { count: cell.runs })}
                              {cell.fps === null ? "" : ` · ${t("ab.fps", { fps: Math.round(cell.fps) })}`}
                            </span>
                            {broke ? <b className={versions.brokeLabel}>{t("versions.broke")}</b> : null}
                          </button>
                        ) : (
                          <span className={styles.muted}>—</span>
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </Page>
  );
}
