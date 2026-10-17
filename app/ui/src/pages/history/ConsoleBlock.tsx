import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { GameSummary } from "@/shared/api/bindings/GameSummary";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import controls from "@/shared/ui/controls.module.css";
import styles from "@/shared/ui/lab.module.css";
import history from "./HistoryPage.module.css";

export function wantsConsole(run: RunSummary, game: GameSummary): boolean {
  return game.outcome === "console" || (run.buildOnly && game.titleId !== "");
}

export function waitsForConsole(run: RunSummary, game: GameSummary): boolean {
  return wantsConsole(run, game) && !run.console.some((verdict) => verdict.game === game.dir);
}

type ConsoleBlockProps = {
  run: RunSummary;
  game: GameSummary;
  onOpen: (...rest: string[]) => void;
  onSaved: () => void;
};

export function ConsoleBlock({ run, game, onOpen, onSaved }: ConsoleBlockProps) {
  const { t } = useTranslation();
  const verdict = run.console.find((entry) => entry.game === game.dir) ?? null;
  const [note, setNote] = useState("");
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!wantsConsole(run, game) && verdict === null) {
    return null;
  }
  const save = (works: boolean) => {
    setError(null);
    api
      .recordConsoleVerdict(run.dir, game.dir, works, note)
      .then(() => {
        setEditing(false);
        setNote("");
        onSaved();
      })
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  if (verdict && !editing) {
    return (
      <div className={history.console}>
        <span className={styles.label}>{t("console.title")}</span>
        <span className={styles.tags}>
          <Chip tone={verdict.works ? "ok" : "bad"}>{t(verdict.works ? "console.works" : "console.fails")}</Chip>
          <span className={styles.muted}>{verdict.at}</span>
        </span>
        {verdict.note ? <span>{verdict.note}</span> : null}
        <div className={styles.actions}>
          <Button variant="ghost" onClick={() => setEditing(true)}>
            {t("console.change")}
          </Button>
        </div>
      </div>
    );
  }
  return (
    <div className={history.console}>
      <span className={styles.label}>{t("console.title")}</span>
      <span className={styles.muted}>{t(run.buildOnly ? "console.builtHint" : "console.gapHint")}</span>
      <input
        className={controls.input}
        aria-label={t("console.note")}
        placeholder={t("console.note")}
        value={note}
        onChange={(event) => setNote(event.target.value)}
      />
      {error ? <Notice tone="error">{error}</Notice> : null}
      <div className={styles.actions}>
        <Button variant="primary" onClick={() => save(true)}>
          <Icon name="check" size={14} />
          {t("console.works")}
        </Button>
        <Button variant="danger" onClick={() => save(false)}>
          <Icon name="alert" size={14} />
          {t("console.fails")}
        </Button>
        {run.buildOnly && game.dir ? (
          <Button variant="ghost" onClick={() => onOpen(game.dir)}>
            <Icon name="folder" size={14} />
            {t("console.showNsp")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
