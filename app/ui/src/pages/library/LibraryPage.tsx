import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { lastResult, outcomeTone } from "@/features/lab/outcome";
import { settingsSummary } from "@/features/lab/settingsSummary";
import { StepLine } from "@/features/lab/StepLine";
import type { GameView } from "@/shared/api/bindings/GameView";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { GameArt } from "@/shared/ui/GameArt";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";

export function LibraryPage() {
  const { t } = useTranslation();
  const { lab, runs } = useLab();
  const [picked, setPicked] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [notice, setNotice] = useState<{ tone: "info" | "error"; text: string } | null>(null);
  const games = lab.data?.games ?? [];
  const selected = games.find((game) => game.name === picked) ?? games[0] ?? null;
  const refreshLab = lab.refresh;

  const add = useCallback(
    async (folders: string[]) => {
      for (const folder of folders) {
        try {
          const name = await api.addGame(folder);
          setNotice({ tone: "info", text: t("library.added", { name }) });
          setPicked(name);
        } catch (reason) {
          setNotice({ tone: "error", text: errorText(reason) });
        }
      }
      refreshLab();
    },
    [refreshLab, t],
  );

  useEffect(() => {
    let active = true;
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      if (!active) return;
      if (event.payload.type === "enter" || event.payload.type === "over") {
        setDragging(true);
      } else if (event.payload.type === "leave") {
        setDragging(false);
      } else if (event.payload.type === "drop") {
        setDragging(false);
        void add(event.payload.paths);
      }
    });
    return () => {
      active = false;
      void pending.then((unlisten) => unlisten());
    };
  }, [add]);

  const choose = async () => {
    const folder = await open({ directory: true, multiple: false, title: t("library.add") });
    if (typeof folder === "string") {
      await add([folder]);
    }
  };

  const aside = (
    <>
      <span className={styles.muted}>{t("library.count", { count: games.length })}</span>
      <Button onClick={() => void choose()}>
        <Icon name="plus" size={14} />
        {t("library.add")}
      </Button>
    </>
  );
  return (
    <Page title={t("nav.library")} aside={aside}>
      {lab.error ? <Notice tone="error">{lab.error}</Notice> : null}
      {notice ? <Notice tone={notice.tone}>{notice.text}</Notice> : null}
      <div className={dragging ? `${styles.drop} ${styles.dropActive}` : styles.drop}>{t("library.drop")}</div>
      <div className={styles.split}>
        <div className={styles.list}>
          {games.length === 0 && !lab.error ? <div className={styles.empty}>{t("library.empty")}</div> : null}
          {games.map((game) => (
            <GameRow
              key={game.name}
              game={game}
              runs={runs.data ?? []}
              selected={game.name === selected?.name}
              onPick={() => setPicked(game.name)}
            />
          ))}
        </div>
        {selected ? <GameCard game={selected} runs={runs.data ?? []} /> : null}
      </div>
    </Page>
  );
}

function GameRow({ game, runs, selected, onPick }: { game: GameView; runs: RunSummary[]; selected: boolean; onPick: () => void }) {
  const { t } = useTranslation();
  const last = lastResult(runs, game.name);
  return (
    <button type="button" className={selected ? `${styles.row} ${styles.selected}` : styles.row} onClick={onPick}>
      <GameArt title={game.title ?? game.name} size="row" />
      <span className={styles.name}>
        <b>{game.name}</b>
        <span className={styles.tags}>
          {game.enabled ? null : <Chip>{t("library.off")}</Chip>}
          {game.sets.map((set) => (
            <Chip key={set} tone="acc">
              {set}
            </Chip>
          ))}
          <Chip>{t("library.steps", { count: game.scenario.steps.length })}</Chip>
        </span>
      </span>
      <span className={styles.end}>
        {last ? (
          <>
            <Chip tone={outcomeTone(last.outcome)}>{t(`outcome.${last.outcome}`)}</Chip>
            <span>{last.run.started}</span>
          </>
        ) : (
          <span>{t("library.neverRun")}</span>
        )}
      </span>
    </button>
  );
}

function GameCard({ game, runs }: { game: GameView; runs: RunSummary[] }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { status } = useLab();
  const [error, setError] = useState<string | null>(null);
  const last = lastResult(runs, game.name);
  const run = () => {
    setError(null);
    api
      .startRun(null, game.name)
      .then(() => {
        status.refresh();
        navigate("/run");
      })
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  return (
    <aside className={styles.card}>
      <div className={styles.cardHead}>
        <GameArt title={game.title ?? game.name} size="row" />
        <h2>{game.name}</h2>
      </div>
      <dl className={styles.kv}>
        <dt>{t("library.titleId")}</dt>
        <dd className={styles.mono}>{game.titleId}</dd>
        <dt>{t("library.folder")}</dt>
        <dd>{game.folder}</dd>
        {game.executable ? (
          <>
            <dt>{t("library.executable")}</dt>
            <dd>{game.executable}</dd>
          </>
        ) : null}
        <dt>{t("library.settings")}</dt>
        <dd>{settingsSummary(game.settings).join(" · ")}</dd>
        <dt>{t("library.scenario")}</dt>
        <dd>{game.ownScenario ? t("library.ownScenario", { count: game.scenario.steps.length }) : t("library.standardScenario")}</dd>
        <dt>{t("library.last")}</dt>
        <dd>
          {last ? (
            <Chip tone={outcomeTone(last.outcome)}>
              {t(`outcome.${last.outcome}`)} · {last.run.started}
            </Chip>
          ) : (
            t("library.neverRun")
          )}
        </dd>
      </dl>
      {error ? <Notice tone="error">{error}</Notice> : null}
      <div className={styles.actions}>
        <Button variant="primary" onClick={run}>
          <Icon name="play" size={14} />
          {t("library.run")}
        </Button>
        <Button onClick={() => void api.openPath(game.folder).catch((reason: unknown) => setError(errorText(reason)))}>
          <Icon name="folder" size={14} />
          {t("library.openFolder")}
        </Button>
        {last ? (
          <Button variant="ghost" onClick={() => navigate(`/history/${last.run.dir}`)}>
            {t("library.openRun")}
          </Button>
        ) : null}
      </div>
      <span className={styles.label}>{t("library.scenario")}</span>
      <ol className={styles.steps}>
        {game.scenario.steps.slice(0, 12).map((step, index) => (
          <StepLine key={index} step={step} />
        ))}
      </ol>
      {game.scenario.steps.length > 12 ? (
        <span className={styles.muted}>{t("library.moreSteps", { count: game.scenario.steps.length - 12 })}</span>
      ) : null}
    </aside>
  );
}
