import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { GameView } from "@/shared/api/bindings/GameView";
import type { Release } from "@/shared/api/bindings/Release";
import { api } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Icon } from "@/shared/ui/Icon";
import styles from "@/shared/ui/lab.module.css";
import plans from "./PlansPage.module.css";

const PRESETS: { key: string; json: string }[] = [
  { key: "box64", json: '{ "system": { "cpu": "box64" } }' },
  { key: "fex", json: '{ "system": { "cpu": "fex" } }' },
  { key: "gplasync", json: '{ "graphics": { "dxvkSource": "gplasync" } }' },
  { key: "twoCores", json: '{ "system": { "fourCores": false } }' },
  { key: "noVsync", json: '{ "graphics": { "vsync": false } }' },
  { key: "hudOff", json: '{ "graphics": { "hud": "off" } }' },
];

export type VariantB = { overrides: string } | { runtime: string };

type AbPlanProps = {
  games: GameView[];
  onStart: (game: string, variant: VariantB, repeat: number) => void;
};

export function AbPlan({ games, onStart }: AbPlanProps) {
  const { t } = useTranslation();
  const [game, setGame] = useState<string | null>(null);
  const [mode, setMode] = useState<"settings" | "runtime">("settings");
  const [overrides, setOverrides] = useState(PRESETS[0]?.json ?? "{}");
  const [releases, setReleases] = useState<Release[]>([]);
  const [release, setRelease] = useState<string | null>(null);
  const [repeat, setRepeat] = useState(3);
  useEffect(() => {
    api
      .listReleases()
      .then((list) => {
        setReleases(list);
        setRelease((current) => current ?? list.find((entry) => entry.kept)?.tag ?? null);
      })
      .catch(() => setReleases([]));
  }, []);
  let valid = true;
  try {
    const parsed: unknown = JSON.parse(overrides);
    valid = typeof parsed === "object" && parsed !== null && !Array.isArray(parsed);
  } catch {
    valid = false;
  }
  const ready = mode === "settings" ? valid : release !== null;
  const start = () => {
    if (!game) {
      return;
    }
    onStart(game, mode === "settings" ? { overrides } : { runtime: `release:${release ?? "latest"}` }, repeat);
  };
  return (
    <section className={styles.card}>
      <div className={styles.cardHead}>
        <h2>{t("ab.title")}</h2>
      </div>
      <p className={styles.muted}>{t(mode === "settings" ? "ab.hint" : "ab.runtimeHint")}</p>
      <div className={plans.choices}>
        {games.map((entry) => (
          <label key={entry.name} className={plans.choice} htmlFor={`ab-${entry.name}`}>
            <input
              id={`ab-${entry.name}`}
              type="radio"
              name="ab-game"
              checked={game === entry.name}
              onChange={() => setGame(entry.name)}
            />
            <span>{entry.name}</span>
          </label>
        ))}
      </div>
      <span className={styles.label}>{t("ab.variantB")}</span>
      <div className={plans.choices}>
        <Button variant={mode === "settings" ? "primary" : "default"} onClick={() => setMode("settings")}>
          {t("ab.otherSettings")}
        </Button>
        <Button variant={mode === "runtime" ? "primary" : "default"} onClick={() => setMode("runtime")}>
          {t("ab.otherRuntime")}
        </Button>
      </div>
      {mode === "settings" ? (
        <>
          <div className={plans.choices}>
            {PRESETS.map((preset) => (
              <Button
                key={preset.key}
                variant={overrides === preset.json ? "primary" : "default"}
                onClick={() => setOverrides(preset.json)}
              >
                {t(`ab.presets.${preset.key}`)}
              </Button>
            ))}
          </div>
          <textarea
            id="ab-overrides"
            className={valid ? plans.json : `${plans.json} ${plans.invalid}`}
            rows={3}
            spellCheck={false}
            value={overrides}
            onChange={(event) => setOverrides(event.target.value)}
          />
        </>
      ) : (
        <div className={plans.choices}>
          {releases.length === 0 ? <span className={styles.muted}>{t("ab.noReleases")}</span> : null}
          {releases.map((entry) => (
            <label
              key={entry.tag}
              className={plans.choice}
              htmlFor={`ab-release-${entry.tag}`}
              title={entry.kept ? undefined : t("ab.notBuilt", { tag: entry.tag })}
            >
              <input
                id={`ab-release-${entry.tag}`}
                type="radio"
                name="ab-release"
                disabled={!entry.kept}
                checked={release === entry.tag}
                onChange={() => setRelease(entry.tag)}
              />
              <span>{entry.kept ? entry.tag : t("ab.notBuiltShort", { tag: entry.tag })}</span>
            </label>
          ))}
        </div>
      )}
      <div className={plans.footer}>
        <label className={plans.repeat} htmlFor="ab-repeat">
          {t("ab.repeat")}
          <input
            id="ab-repeat"
            type="number"
            min={1}
            max={50}
            value={repeat}
            onChange={(event) => setRepeat(Math.min(50, Math.max(1, Number(event.target.value) || 1)))}
          />
        </label>
        <span className={plans.math}>{game ? t("ab.launches", { count: repeat * 2 }) : t("ab.pickGame")}</span>
        <Button variant="primary" disabled={!game || !ready} onClick={start}>
          <Icon name="play" size={14} />
          {t("ab.start")}
        </Button>
      </div>
    </section>
  );
}
