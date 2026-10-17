import { useTranslation } from "react-i18next";
import type { PerfSample } from "@/shared/api/bindings/PerfSample";
import styles from "./FpsChart.module.css";

const WIDTH = 300;
const HEIGHT = 64;

export function FpsChart({ samples }: { samples: PerfSample[] }) {
  const { t } = useTranslation();
  const drawing = samples.filter((sample) => sample.fps > 0);
  const last = samples[samples.length - 1];
  const top = Math.max(60, ...drawing.map((sample) => sample.fps));
  const first = samples[0]?.at ?? 0;
  const span = Math.max(1, (last?.at ?? 0) - first);
  const points = samples.map((sample) => {
    const x = ((sample.at - first) / span) * WIDTH;
    const y = HEIGHT - (sample.fps / top) * (HEIGHT - 4) - 2;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  const thirty = HEIGHT - (30 / top) * (HEIGHT - 4) - 2;
  return (
    <div className={styles.chart}>
      <div className={styles.top}>
        <span>{t("fps.title")}</span>
        <b>{last ? t("fps.now", { fps: Math.round(last.fps), speed: Math.round(last.speed * 100) }) : "—"}</b>
      </div>
      <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none" role="img" aria-label={t("fps.title")}>
        <line className={styles.grid} x1={0} x2={WIDTH} y1={thirty} y2={thirty} />
        {points.length > 1 ? (
          <>
            <polygon className={styles.area} points={`0,${HEIGHT} ${points.join(" ")} ${WIDTH},${HEIGHT}`} />
            <polyline className={styles.line} points={points.join(" ")} />
          </>
        ) : null}
      </svg>
      <span className={styles.note}>{t("fps.mark")}</span>
    </div>
  );
}
