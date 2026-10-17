import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Notice } from "@/shared/ui/Notice";
import styles from "@/shared/ui/lab.module.css";
import history from "./HistoryPage.module.css";

function shotName(path: string): string {
  const file = path.split(/[\\/]/).pop() ?? path;
  return file.replace(/\.png$/i, "");
}

export function ShotsBlock({ dir, game }: { dir: string; game: string }) {
  const { t } = useTranslation();
  const [shots, setShots] = useState<string[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [picture, setPicture] = useState<string | null>(null);
  const [made, setMade] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api
      .runScreenshots(dir, game)
      .then(setShots)
      .catch(() => setShots([]));
  }, [dir, game]);
  useEffect(() => {
    setPicture(null);
    setMade(null);
    if (open) {
      api
        .readImage(open)
        .then(setPicture)
        .catch(() => setPicture(null));
    }
  }, [open]);
  if (shots.length === 0) {
    return null;
  }
  const makeReference = (path: string) => {
    setError(null);
    api
      .makeReference(dir, game, shotName(path))
      .then(setMade)
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  return (
    <div className={history.shots}>
      <span className={styles.label}>{t("history.shots")}</span>
      <span className={styles.tags}>
        {shots.map((path) => (
          <Button key={path} variant={open === path ? "primary" : "ghost"} onClick={() => setOpen(open === path ? null : path)}>
            {shotName(path)}
          </Button>
        ))}
      </span>
      {open && picture ? (
        <>
          <img className={history.shot} src={picture} alt={shotName(open)} />
          <div className={styles.actions}>
            <Button onClick={() => makeReference(open)}>{t("history.makeReference")}</Button>
            {made ? <span className={styles.muted}>{t("history.referenceMade", { path: made })}</span> : null}
          </div>
        </>
      ) : null}
      {error ? <Notice tone="error">{error}</Notice> : null}
    </div>
  );
}
