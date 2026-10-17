import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { outcomeTone, runCounts, runPath } from "@/features/lab/outcome";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";

export function HistoryPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { dir } = useParams();
  const { runs } = useLab();
  const list = runs.data ?? [];
  const selected = list.find((run) => run.dir === dir) ?? list[0] ?? null;
  return (
    <Page title={t("nav.history")} aside={<span className={styles.muted}>{t("history.count", { count: list.length })}</span>}>
      {runs.error ? <Notice tone="error">{runs.error}</Notice> : null}
      <div className={styles.split}>
        <div className={styles.list}>
          {list.length === 0 ? <div className={styles.empty}>{t("history.empty")}</div> : null}
          {list.map((run) => (
            <RunRow key={run.dir} run={run} selected={run.dir === selected?.dir} onPick={() => navigate(`/history/${run.dir}`)} />
          ))}
        </div>
        {selected ? <RunCard run={selected} /> : null}
      </div>
    </Page>
  );
}

function RunRow({ run, selected, onPick }: { run: RunSummary; selected: boolean; onPick: () => void }) {
  const { t } = useTranslation();
  const counts = runCounts(run);
  return (
    <button type="button" className={`${styles.row} ${styles.plainRow} ${selected ? styles.selected : ""}`} onClick={onPick}>
      <span className={styles.name}>
        <b>{run.started}</b>
        <span className={styles.tags}>
          {run.games.map((game, index) => (
            <Chip key={index} tone={outcomeTone(game.outcome)}>
              {game.name}
            </Chip>
          ))}
        </span>
      </span>
      <span className={styles.end}>
        {run.finished ? null : <Chip tone="acc">{t("history.notFinished")}</Chip>}
        <span>{t("history.counts", counts)}</span>
        <span>{t("units.minutes", { count: Math.max(1, Math.round(run.seconds / 60)) })}</span>
      </span>
    </button>
  );
}

function RunCard({ run }: { run: RunSummary }) {
  const { t } = useTranslation();
  const { status } = useLab();
  const [error, setError] = useState<string | null>(null);
  const work = status.data?.work ?? "";
  const open = (...rest: string[]) => {
    setError(null);
    api.openPath(runPath(work, run.dir, ...rest)).catch((reason: unknown) => setError(errorText(reason)));
  };
  return (
    <aside className={styles.card}>
      <div className={styles.cardHead}>
        <h2>{run.started}</h2>
      </div>
      <dl className={styles.kv}>
        <dt>{t("history.folder")}</dt>
        <dd className={styles.mono}>{run.dir}</dd>
        <dt>{t("history.emulator")}</dt>
        <dd>{run.emulator}</dd>
        <dt>{t("history.runtime")}</dt>
        <dd>{run.runtimeVersion}</dd>
        <dt>{t("history.took")}</dt>
        <dd>{t("units.minutes", { count: Math.max(1, Math.round(run.seconds / 60)) })}</dd>
        <dt>{t("history.planned")}</dt>
        <dd>{t("history.done", { done: run.games.length, planned: run.planned.length })}</dd>
      </dl>
      <span className={styles.label}>{t("run.games")}</span>
      <div className={styles.list}>
        {run.games.map((game, index) => (
          <div key={index} className={`${styles.row} ${styles.plainRow}`}>
            <span className={styles.name}>
              <b>{game.name}</b>
              <span className={styles.muted}>{game.settings}</span>
            </span>
            <Chip tone={outcomeTone(game.outcome)}>{t(`outcome.${game.outcome}`)}</Chip>
          </div>
        ))}
      </div>
      {error ? <Notice tone="error">{error}</Notice> : null}
      <div className={styles.actions}>
        <Button variant="primary" onClick={() => open("report.html")}>
          <Icon name="external" size={14} />
          {t("run.report")}
        </Button>
        <Button onClick={() => open()}>
          <Icon name="folder" size={14} />
          {t("history.openFolder")}
        </Button>
      </div>
    </aside>
  );
}
