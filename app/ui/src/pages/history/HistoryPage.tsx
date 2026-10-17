import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { outcomeTone, runCounts, runPath, type Tone } from "@/features/lab/outcome";
import { folderName } from "@/features/lab/useRunEvents";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";
import { VerdictBlock } from "./VerdictBlock";

export function HistoryPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { dir } = useParams();
  const { runs } = useLab();
  const [marked, setMarked] = useState<string[]>([]);
  const list = runs.data ?? [];
  const selected = list.find((run) => run.dir === dir) ?? list[0] ?? null;
  const mark = (runDir: string) =>
    setMarked((current) =>
      current.includes(runDir) ? current.filter((entry) => entry !== runDir) : [...current, runDir].slice(-2),
    );
  const [older, newer] = [...marked].sort();
  const aside = (
    <>
      <span className={styles.muted}>{t("history.count", { count: list.length })}</span>
      <Button disabled={marked.length !== 2} onClick={() => navigate(`/compare/${older}/${newer}`)}>
        {t("history.compare", { count: marked.length })}
      </Button>
    </>
  );
  return (
    <Page title={t("nav.history")} aside={aside}>
      {runs.error ? <Notice tone="error">{runs.error}</Notice> : null}
      <div className={styles.split}>
        <div className={styles.list}>
          {list.length === 0 ? <div className={styles.empty}>{t("history.empty")}</div> : null}
          {list.map((run) => (
            <RunRow
              key={run.dir}
              run={run}
              selected={run.dir === selected?.dir}
              marked={marked.includes(run.dir)}
              onMark={() => mark(run.dir)}
              onPick={() => navigate(`/history/${run.dir}`)}
            />
          ))}
        </div>
        {selected ? <RunCard run={selected} /> : null}
      </div>
    </Page>
  );
}

function RunCounts({ run }: { run: RunSummary }) {
  const { t } = useTranslation();
  const counts = runCounts(run);
  const shown: { key: keyof typeof counts; tone: Tone }[] = [
    { key: "passed", tone: "ok" },
    { key: "warned", tone: "warn" },
    { key: "console", tone: "acc" },
    { key: "failed", tone: "bad" },
  ];
  return (
    <span className={styles.tags}>
      {shown
        .filter(({ key }) => counts[key] > 0)
        .map(({ key, tone }) => (
          <Chip key={key} tone={tone}>
            {t(`history.${key}`, { count: counts[key] })}
          </Chip>
        ))}
    </span>
  );
}

function RunState({ run }: { run: RunSummary }) {
  const { t } = useTranslation();
  const { status } = useLab();
  if (run.finished) {
    return null;
  }
  const holderDir = status.data?.queue.holder?.runDir ?? null;
  const running = holderDir !== null && folderName(holderDir) === run.dir;
  return running ? <Chip tone="acc">{t("history.running")}</Chip> : <Chip tone="bad">{t("history.broken")}</Chip>;
}

function RunRow({
  run,
  selected,
  marked,
  onMark,
  onPick,
}: {
  run: RunSummary;
  selected: boolean;
  marked: boolean;
  onMark: () => void;
  onPick: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      role="button"
      tabIndex={0}
      className={`${styles.row} ${styles.markRow} ${selected ? styles.selected : ""}`}
      onClick={onPick}
      onKeyDown={(event) => {
        if (event.key === "Enter") onPick();
      }}
    >
      <input
        type="checkbox"
        aria-label={t("history.mark")}
        checked={marked}
        onClick={(event) => event.stopPropagation()}
        onChange={onMark}
      />
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
        <RunState run={run} />
        <RunCounts run={run} />
        <span>{t("units.minutes", { count: Math.max(1, Math.round(run.seconds / 60)) })}</span>
      </span>
    </div>
  );
}

function RunCard({ run }: { run: RunSummary }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { status } = useLab();
  const [error, setError] = useState<string | null>(null);
  const work = status.data?.work ?? "";
  const open = (...rest: string[]) => {
    setError(null);
    api.openPath(runPath(work, run.dir, ...rest)).catch((reason: unknown) => setError(errorText(reason)));
  };
  const repeatRun = () => {
    setError(null);
    api
      .startRun(null, null, run.planned, 1)
      .then(() => {
        status.refresh();
        navigate("/run");
      })
      .catch((reason: unknown) => setError(errorText(reason)));
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
        <dt>{t("history.carafe")}</dt>
        <dd className={styles.mono}>{run.stamp.carafeCommit ?? "—"}</dd>
        <dt>{t("history.runtimeHash")}</dt>
        <dd className={styles.mono}>{run.stamp.runtimeHash ?? "—"}</dd>
        <dt>{t("history.lab")}</dt>
        <dd className={styles.mono}>{run.stamp.labCommit ?? "—"}</dd>
        <dt>{t("history.edenImage")}</dt>
        <dd className={styles.mono}>{run.stamp.edenImage ?? "—"}</dd>
      </dl>
      {run.verdict ? <VerdictBlock verdict={run.verdict} /> : null}
      <span className={styles.label}>{t("run.games")}</span>
      <div className={styles.list}>
        {run.games.map((game, index) => (
          <div key={index} className={`${styles.row} ${styles.plainRow}`}>
            <span className={styles.name}>
              <b>{game.name}</b>
              <span className={`${styles.muted} ${styles.oneLine}`} title={game.settings}>
                {game.settings}
              </span>
              {game.perf ? (
                <span className={styles.muted}>
                  {t("fps.summary", { median: Math.round(game.perf.medianFps), low: Math.round(game.perf.lowFps) })}
                </span>
              ) : null}
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
        <Button variant="ghost" disabled={run.planned.length === 0 || run.verdict !== null} onClick={repeatRun}>
          <Icon name="restart" size={14} />
          {t("history.repeat")}
        </Button>
      </div>
    </aside>
  );
}
