import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { outcomeTone, runPath } from "@/features/lab/outcome";
import { StepLine } from "@/features/lab/StepLine";
import { folderName, useRunEvents } from "@/features/lab/useRunEvents";
import type { Event } from "@/shared/api/bindings/Event";
import type { Outcome } from "@/shared/api/bindings/Outcome";
import type { Stage } from "@/shared/api/bindings/Stage";
import type { QueueEntry } from "@/shared/api/bindings/QueueEntry";
import type { StepResult } from "@/shared/api/bindings/StepResult";
import { api, errorText } from "@/shared/api/commands";
import { usePolling } from "@/shared/api/usePolling";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import { Spinner } from "@/shared/ui/Spinner";
import styles from "@/shared/ui/lab.module.css";
import run from "./RunPage.module.css";

const LIVE_SCREEN = "http://127.0.0.1:6080/carafe.html";

type GameState = { name: string; outcome: Outcome | null; current: boolean };

type Derived = {
  games: GameState[];
  steps: StepResult[];
  stage: string | null;
  launchStage: Stage | null;
  finished: boolean;
};

function derive(events: Event[], planned: string[]): Derived {
  const games: GameState[] = planned.map((name) => ({ name, outcome: null, current: false }));
  let steps: StepResult[] = [];
  let stage: string | null = null;
  let launchStage: Stage | null = null;
  let finished = false;
  for (const event of events) {
    switch (event.event) {
      case "gameStarted": {
        steps = [];
        stage = null;
        const known = games.find((game) => game.name === event.name);
        games.forEach((game) => {
          game.current = false;
        });
        if (known) known.current = true;
        else games.push({ name: event.name, outcome: null, current: true });
        break;
      }
      case "buildStage":
        stage = event.stage;
        break;
      case "built":
        stage = event.reused ? "reused" : "built";
        break;
      case "buildFailed":
        stage = "failed";
        break;
      case "step":
        steps = [...steps, event.step];
        if (event.step.step.step === "launch") launchStage = "notStarted";
        break;
      case "reached":
        launchStage = event.stage;
        break;
      case "gameFinished": {
        const done = games.find((game) => game.name === event.name);
        if (done) {
          done.outcome = event.outcome;
          done.current = false;
        }
        break;
      }
      case "runFinished":
      case "stopped":
        finished = true;
        break;
      default:
        break;
    }
  }
  return { games, steps, stage, launchStage, finished };
}

const STAGES: Stage[] = ["overlay", "wine", "loadingScreen", "firstFrame"];

function StageBar({ stage }: { stage: Stage }) {
  const { t } = useTranslation();
  const reached = STAGES.indexOf(stage);
  return (
    <ol className={run.stages}>
      {STAGES.map((name, index) => (
        <li key={name} className={index <= reached ? run.stageDone : index === reached + 1 ? run.stageNext : undefined}>
          <i />
          <span>{t(`stages.launch.${name}`)}</span>
        </li>
      ))}
    </ol>
  );
}

export function RunPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { status, runs } = useLab();
  const [error, setError] = useState<string | null>(null);
  const queue = status.data?.queue;
  const holder = queue?.holder ?? null;
  const liveDir = holder?.runDir ? folderName(holder.runDir) : null;
  const shownDir = liveDir ?? (holder ? null : (runs.data?.[0]?.dir ?? null));
  const live = holder !== null;
  const events = useRunEvents(shownDir, live);
  const summary = runs.data?.find((entry) => entry.dir === shownDir) ?? null;
  const derived = derive(events, summary?.planned ?? []);
  const screen = usePolling(api.readScreen, 0);
  const [control, setControl] = useState(false);
  const [reloads, setReloads] = useState(0);
  const launches = events.filter(
    (event) => event.event === "step" && event.step.step.step === "launch" && event.step.outcome === "pass",
  ).length;
  const showLive = liveDir !== null && launches > 0 && !derived.finished;
  const stop = (entry: QueueEntry) => {
    setError(null);
    api
      .stopRun(entry.id)
      .then(() => status.refresh())
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  const work = status.data?.work ?? "";
  const aside = live ? (
    <Chip tone="acc">{t("run.live", { who: t(`who.${holder?.who ?? ""}`, { defaultValue: holder?.who ?? "" }) })}</Chip>
  ) : shownDir ? (
    <Chip>{t("run.last", { started: summary?.started ?? shownDir })}</Chip>
  ) : null;
  return (
    <Page title={t("nav.run")} aside={aside}>
      {error ? <Notice tone="error">{error}</Notice> : null}
      {!holder && !shownDir ? (
        <div className={styles.card}>
          <p className={styles.muted}>{t("run.nothing")}</p>
          <div className={styles.actions}>
            <Button variant="primary" onClick={() => navigate("/plans")}>
              {t("run.toPlans")}
            </Button>
          </div>
        </div>
      ) : null}
      {holder || shownDir ? (
      <div className={run.live}>
        <div className={run.column}>
          <span className={styles.label}>{t("run.queue")}</span>
          {holder ? <QueueItem entry={holder} now onStop={() => stop(holder)} /> : <span className={styles.muted}>{t("run.free")}</span>}
          {(queue?.waiting ?? []).map((entry) => (
            <QueueItem key={entry.id} entry={entry} onStop={() => stop(entry)} />
          ))}
          {derived.games.length > 0 ? <span className={styles.label}>{t("run.games")}</span> : null}
          {derived.games.map((game) => (
            <div key={game.name} className={game.current ? `${run.game} ${run.current}` : run.game}>
              <span>{game.name}</span>
              {game.outcome ? (
                <Chip tone={outcomeTone(game.outcome)}>{t(`outcome.${game.outcome}`)}</Chip>
              ) : game.current ? (
                <Spinner />
              ) : null}
            </div>
          ))}
        </div>
        <div className={run.column}>
          <div className={run.switch}>
            <span className={run.joyLeft} />
            <div className={run.screen}>
              {showLive ? (
                <iframe
                  key={`${liveDir}-${launches}-${control}-${reloads}`}
                  title={t("run.screen")}
                  src={control ? LIVE_SCREEN : `${LIVE_SCREEN}?view_only`}
                />
              ) : screen.data ? (
                <img src={screen.data.url} alt={t("run.screen")} />
              ) : (
                <span>{t("run.noScreen")}</span>
              )}
            </div>
            <span className={run.joyRight} />
          </div>
          {showLive ? (
            <div className={styles.actions}>
              <Button variant={control ? "primary" : "default"} onClick={() => setControl(!control)}>
                {control ? t("run.giveBack") : t("run.takeControl")}
              </Button>
              <Button variant="ghost" onClick={() => setReloads(reloads + 1)}>
                <Icon name="restart" size={14} />
                {t("run.reload")}
              </Button>
              <span className={styles.muted}>{control ? t("run.controlHint") : t("run.watchHint")}</span>
            </div>
          ) : null}
          {holder && !liveDir ? (
            <Notice tone="info">{t("run.preparing")}</Notice>
          ) : null}
          {derived.launchStage && live ? <StageBar stage={derived.launchStage} /> : null}
          {derived.launchStage && derived.steps.some((step) => step.step.step === "waitFor" && step.outcome === "fail") ? (
            <Notice tone="warning">{t(`stages.stuck.${derived.launchStage}`)}</Notice>
          ) : null}
          {derived.stage ? <span className={styles.muted}>{t("run.build", { stage: t(`buildStages.${derived.stage}`, { defaultValue: derived.stage }) })}</span> : null}
          <ol className={styles.steps}>
            {derived.steps.map((result, index) => (
              <StepLine
                key={index}
                step={result.step}
                aside={
                  <span className={run.stepEnd}>
                    <span className={styles.muted}>{t("units.at", { seconds: Math.round(result.at) })}</span>
                    <Chip tone={outcomeTone(result.outcome)}>{t(`outcome.${result.outcome}`)}</Chip>
                  </span>
                }
              />
            ))}
          </ol>
          {shownDir && (derived.finished || summary?.finished) ? (
            <div className={styles.actions}>
              <Button onClick={() => void api.openPath(runPath(work, shownDir, "report.html")).catch((reason: unknown) => setError(errorText(reason)))}>
                <Icon name="external" size={14} />
                {t("run.report")}
              </Button>
              <Button variant="ghost" onClick={() => navigate(`/history/${shownDir}`)}>
                {t("run.toHistory")}
              </Button>
            </div>
          ) : null}
        </div>
        <div className={run.column}>
          <span className={styles.label}>{t("run.log")}</span>
          {events.length > 0 ? (
            <div className={run.log}>
              {events.slice(-40).map((event, index) => (
                <span key={index} className={event.event === "buildFailed" || (event.event === "step" && event.step.outcome === "fail") ? run.bad : undefined}>
                  {describe(event, t)}
                </span>
              ))}
            </div>
          ) : (
            <span className={styles.muted}>{t("run.noEvents")}</span>
          )}
        </div>
      </div>
      ) : null}
    </Page>
  );
}

function QueueItem({ entry, now, onStop }: { entry: QueueEntry; now?: boolean; onStop: () => void }) {
  const { t } = useTranslation();
  return (
    <div className={now ? `${run.item} ${run.now}` : run.item}>
      <b>{entry.what}</b>
      <span className={styles.muted}>{t("run.by", { who: t(`who.${entry.who}`, { defaultValue: entry.who }) })}</span>
      <Button variant="danger" onClick={onStop}>
        <Icon name="stop" size={12} />
        {now ? t("run.stop") : t("run.leave")}
      </Button>
    </div>
  );
}

function describe(event: Event, t: (key: string, options?: Record<string, unknown>) => string): string {
  switch (event.event) {
    case "waiting":
      return t("log.waiting", { holder: event.holder || "—", ahead: event.ahead });
    case "runStarted":
      return t("log.runStarted", { games: event.games });
    case "gameStarted":
      return t("log.gameStarted", { index: event.index, name: event.name });
    case "buildStage":
      return t("log.buildStage", { stage: t(`buildStages.${event.stage}`, { defaultValue: event.stage }) });
    case "built":
      return event.reused ? t("log.reused", { titleId: event.titleId }) : t("log.built", { titleId: event.titleId, seconds: Math.round(event.seconds) });
    case "buildFailed":
      return t("log.buildFailed", { error: event.error });
    case "launchLogs":
      return t("log.launch");
    case "reached":
      return t("log.reached", { stage: t(`stages.launch.${event.stage}`) });
    case "step":
      return `${Math.round(event.step.at)} s · ${t(`steps.${event.step.step.step}`)} · ${t(`outcome.${event.step.outcome}`)} · ${event.step.detail}`;
    case "gameFinished":
      return t("log.gameFinished", { name: event.name, outcome: t(`outcome.${event.outcome}`) });
    case "stopped":
      return t("log.stopped", { left: event.left });
    case "runFinished":
      return t("log.runFinished", { passed: event.passed, warned: event.warned, console: event.console, failed: event.failed });
  }
}
