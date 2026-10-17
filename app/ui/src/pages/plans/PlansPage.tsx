import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { lastSeconds } from "@/features/lab/outcome";
import type { GameView } from "@/shared/api/bindings/GameView";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";
import plans from "./PlansPage.module.css";

type Estimate = { launches: number; seconds: number; unknown: number };

function estimate(chosen: GameView[], runs: RunSummary[], repeat: number): Estimate {
  let launches = 0;
  let seconds = 0;
  let unknown = 0;
  for (const game of chosen) {
    launches += game.scenario.steps.filter((step) => step.step === "launch").length * repeat;
    const known = lastSeconds(runs, game.name);
    if (known === null) {
      unknown += 1;
    } else {
      seconds += known * repeat;
    }
  }
  return { launches, seconds, unknown };
}

export function PlansPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { lab, status, runs } = useLab();
  const [error, setError] = useState<string | null>(null);
  const games = lab.data?.games ?? [];
  const sets = Object.entries(lab.data?.sets ?? {});
  const enabled = games.filter((game) => game.enabled).map((game) => game.name);
  const start = (set: string | null, chosen: string[] | null = null, repeat = 1) => {
    setError(null);
    api
      .startRun(set, null, chosen, repeat)
      .then(() => {
        status.refresh();
        navigate("/run");
      })
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  return (
    <Page title={t("nav.plans")}>
      {lab.error ? <Notice tone="error">{lab.error}</Notice> : null}
      {error ? <Notice tone="error">{error}</Notice> : null}
      <p className={styles.muted}>{t("plans.hint")}</p>
      <OwnPlan games={games} runs={runs.data ?? []} onStart={(chosen, repeat) => start(null, chosen, repeat)} />
      <div className={plans.grid}>
        <PlanCard title={t("plans.enabled")} names={enabled} onStart={() => start(null)} />
        {sets.map(([set, names]) => (
          <PlanCard key={set} title={set} names={names ?? []} onStart={() => start(set)} />
        ))}
      </div>
    </Page>
  );
}

function OwnPlan({
  games,
  runs,
  onStart,
}: {
  games: GameView[];
  runs: RunSummary[];
  onStart: (chosen: string[], repeat: number) => void;
}) {
  const { t } = useTranslation();
  const [chosen, setChosen] = useState<string[]>([]);
  const [repeat, setRepeat] = useState(1);
  const picked = games.filter((game) => chosen.includes(game.name));
  const guess = estimate(picked, runs, repeat);
  const toggle = (name: string) =>
    setChosen((current) => (current.includes(name) ? current.filter((entry) => entry !== name) : [...current, name]));
  return (
    <section className={styles.card}>
      <div className={styles.cardHead}>
        <h2>{t("plans.own")}</h2>
      </div>
      <div className={plans.choices}>
        {games.map((game) => (
          <label key={game.name} className={plans.choice} htmlFor={`plan-${game.name}`}>
            <input
              id={`plan-${game.name}`}
              type="checkbox"
              checked={chosen.includes(game.name)}
              onChange={() => toggle(game.name)}
            />
            <span>{game.name}</span>
          </label>
        ))}
      </div>
      <div className={plans.footer}>
        <label className={plans.repeat} htmlFor="plan-repeat">
          {t("plans.repeat")}
          <input
            id="plan-repeat"
            type="number"
            min={1}
            max={100}
            value={repeat}
            onChange={(event) => setRepeat(Math.min(100, Math.max(1, Number(event.target.value) || 1)))}
          />
        </label>
        <span className={plans.math}>
          {picked.length === 0
            ? t("plans.pickGames")
            : guess.seconds > 0
              ? t("plans.estimate", {
                  launches: guess.launches,
                  minutes: Math.max(1, Math.round(guess.seconds / 60)),
                })
              : t("plans.launches", { launches: guess.launches })}
          {guess.unknown > 0 ? ` · ${t("plans.unknown", { count: guess.unknown })}` : ""}
        </span>
        <Button variant="primary" disabled={picked.length === 0} onClick={() => onStart(chosen, repeat)}>
          <Icon name="play" size={14} />
          {t("plans.start")}
        </Button>
      </div>
    </section>
  );
}

function PlanCard({ title, names, onStart }: { title: string; names: string[]; onStart: () => void }) {
  const { t } = useTranslation();
  return (
    <section className={styles.card}>
      <div className={styles.cardHead}>
        <h2>{title}</h2>
        <Chip>{t("plans.games", { count: names.length })}</Chip>
      </div>
      <ul className={plans.games}>
        {names.map((name) => (
          <li key={name}>{name}</li>
        ))}
      </ul>
      <div className={styles.actions}>
        <Button variant="primary" disabled={names.length === 0} onClick={onStart}>
          <Icon name="play" size={14} />
          {t("plans.start")}
        </Button>
      </div>
    </section>
  );
}
