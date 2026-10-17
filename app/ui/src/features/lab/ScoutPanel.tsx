import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { SwitchButton } from "@/shared/api/bindings/SwitchButton";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Notice } from "@/shared/ui/Notice";
import styles from "@/shared/ui/lab.module.css";
import { KEY_BUTTONS, type Mark, type Pressed, scoutedScenario } from "./scouting";

export function ScoutPanel({ game }: { game: string | null }) {
  const { t } = useTranslation();
  const [recording, setRecording] = useState(false);
  const [presses, setPresses] = useState<Pressed[]>([]);
  const [marks, setMarks] = useState<Mark[]>([]);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const started = useRef(0);
  const downs = useRef(new Map<SwitchButton, number>());

  useEffect(() => {
    if (!recording) {
      return undefined;
    }
    const now = () => performance.now() - started.current;
    const onDown = (event: KeyboardEvent) => {
      const button = KEY_BUTTONS[event.key.toLowerCase()];
      if (button && !event.repeat && !downs.current.has(button)) {
        downs.current.set(button, now());
      }
    };
    const onUp = (event: KeyboardEvent) => {
      const button = KEY_BUTTONS[event.key.toLowerCase()];
      const down = button ? downs.current.get(button) : undefined;
      if (button && down !== undefined) {
        downs.current.delete(button);
        const up = now();
        setPresses((current) => [...current, { button, down, up }]);
      }
    };
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
    };
  }, [recording]);

  const start = () => {
    setPresses([]);
    setMarks([]);
    setSaved(null);
    setError(null);
    downs.current.clear();
    started.current = performance.now();
    setRecording(true);
  };
  const mark = () =>
    setMarks((current) => [...current, { name: `stage-${current.length + 1}`, at: performance.now() - started.current }]);
  const save = () => {
    if (!game) {
      return;
    }
    setRecording(false);
    api
      .saveScenario(game, scoutedScenario(presses, marks))
      .then(setSaved)
      .catch((reason: unknown) => setError(errorText(reason)));
  };

  return (
    <div className={styles.actions}>
      {recording ? (
        <>
          <Button onClick={mark}>{t("scout.mark")}</Button>
          <Button variant="primary" disabled={!game} onClick={save}>
            {t("scout.save")}
          </Button>
          <Button variant="ghost" onClick={() => setRecording(false)}>
            {t("scout.cancel")}
          </Button>
          <span className={styles.muted}>{t("scout.counts", { presses: presses.length, marks: marks.length })}</span>
        </>
      ) : (
        <>
          <Button disabled={!game} onClick={start} title={t("scout.hint")}>
            {t("scout.record")}
          </Button>
          {saved ? <span className={styles.muted}>{t("scout.saved", { file: saved, game })}</span> : null}
        </>
      )}
      {error ? <Notice tone="error">{error}</Notice> : null}
    </div>
  );
}
