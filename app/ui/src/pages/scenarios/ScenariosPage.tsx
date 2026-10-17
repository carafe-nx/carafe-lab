import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLab } from "@/features/lab/LabProvider";
import type { GameView } from "@/shared/api/bindings/GameView";
import type { Step } from "@/shared/api/bindings/Step";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";
import editor from "./ScenariosPage.module.css";
import { NEW_STEPS, newStep, StepEditor } from "./StepEditor";

export function ScenariosPage() {
  const { t } = useTranslation();
  const { lab } = useLab();
  const [picked, setPicked] = useState<string | null>(null);
  const games = lab.data?.games ?? [];
  const selected = games.find((game) => game.name === picked) ?? games[0] ?? null;
  return (
    <Page title={t("nav.scenarios")}>
      {lab.error ? <Notice tone="error">{lab.error}</Notice> : null}
      <p className={styles.muted}>{t("scenarios.hint")}</p>
      <div className={styles.split}>
        {selected ? (
          <Editor key={selected.name} game={selected} onSaved={lab.refresh} />
        ) : (
          <span className={styles.muted}>{t("library.empty")}</span>
        )}
        <div className={styles.list}>
          {games.map((game) => (
            <button
              key={game.name}
              type="button"
              className={`${styles.row} ${styles.plainRow} ${game.name === selected?.name ? styles.selected : ""}`}
              onClick={() => setPicked(game.name)}
            >
              <span className={styles.name}>
                <b>{game.name}</b>
                <span className={styles.muted}>{game.scenarioFile ?? (game.ownScenario ? "lab.json" : t("scenarios.standard"))}</span>
              </span>
              <Chip>{t("library.steps", { count: game.scenario.steps.length })}</Chip>
            </button>
          ))}
        </div>
      </div>
    </Page>
  );
}

function Editor({ game, onSaved }: { game: GameView; onSaved: () => void }) {
  const { t } = useTranslation();
  const [steps, setSteps] = useState<Step[]>(game.scenario.steps);
  const [dirty, setDirty] = useState(false);
  const [notice, setNotice] = useState<{ tone: "info" | "error"; text: string } | null>(null);

  useEffect(() => {
    if (!dirty) {
      setSteps(game.scenario.steps);
    }
  }, [game.scenario.steps, dirty]);

  const shots = steps.flatMap((step) => (step.step === "screenshot" ? [step.name] : []));
  const update = (next: Step[]) => {
    setSteps(next);
    setDirty(true);
    setNotice(null);
  };
  const save = () => {
    api
      .saveScenario(game.name, { steps })
      .then((file) => {
        setDirty(false);
        setNotice({ tone: "info", text: t("scenarios.saved", { file }) });
        onSaved();
      })
      .catch((reason: unknown) => setNotice({ tone: "error", text: errorText(reason) }));
  };
  const move = (index: number, offset: number) => {
    const next = [...steps];
    const [taken] = next.splice(index, 1);
    if (taken) next.splice(index + offset, 0, taken);
    update(next);
  };

  return (
    <div className={styles.card}>
      <div className={styles.cardHead}>
        <h2>{game.name}</h2>
        <Chip tone={game.ownScenario ? "acc" : "plain"}>
          {game.ownScenario ? t("scenarios.own") : t("scenarios.standard")}
        </Chip>
        <span className={editor.spacer} />
        <Button
          variant="ghost"
          disabled={!dirty}
          onClick={() => {
            setSteps(game.scenario.steps);
            setDirty(false);
            setNotice(null);
          }}
        >
          {t("scenarios.reset")}
        </Button>
        <Button variant="primary" disabled={!dirty} onClick={save}>
          {t("scenarios.save")}
        </Button>
      </div>
      {notice ? <Notice tone={notice.tone}>{notice.text}</Notice> : null}
      <ol className={`${styles.steps} ${editor.steps}`}>
        {steps.map((step, index) => (
          <StepEditor
            key={index}
            step={step}
            index={index}
            count={steps.length}
            shots={shots}
            onChange={(changed) => update(steps.map((old, at) => (at === index ? changed : old)))}
            onMove={(offset) => move(index, offset)}
            onRemove={() => update(steps.filter((_, at) => at !== index))}
          />
        ))}
      </ol>
      <span className={styles.label}>{t("scenarios.add")}</span>
      <div className={editor.palette}>
        {NEW_STEPS.map((kind) => (
          <Button key={kind} onClick={() => update([...steps, newStep(kind, steps)])}>
            + {t(`steps.${kind}`)}
          </Button>
        ))}
      </div>
    </div>
  );
}
