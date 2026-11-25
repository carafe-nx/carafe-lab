import { Fragment } from "react";
import { useTranslation } from "react-i18next";
import type { ResourceSummary } from "@/shared/api/bindings/ResourceSummary";
import styles from "@/shared/ui/lab.module.css";
import memory from "./MemoryDetails.module.css";

export function MemoryDetails({ resources }: { resources: ResourceSummary }) {
  const { t } = useTranslation();
  const size = (value: number | null | undefined) => value == null ? "—" : `${value} MiB`;
  const rows = [
    ["guest", `${size(resources.peakGuestMemoryMb)} / ${size(resources.guestLimitMb)}`],
    ["physical", size(resources.guestTotalMb)],
    ["container", `${size(resources.peakContainerMemoryMb)} / ${size(resources.memoryLimitMb)}`],
    ["fileCache", size(resources.peakFileCacheMb)],
    ["gpu", `${size(resources.minGpuAvailableMb)} / ${size(resources.gpuCapacityMb)}`],
    ["cacheTarget", size(resources.gpuCacheBudgetMb)],
    ["upload", size(resources.peakUploadReservedMb)],
    ["download", size(resources.peakDownloadReservedMb)],
    ["oom", resources.oomKills == null ? "—" : String(resources.oomKills)],
  ];
  return (
    <details className={styles.muted}>
      <summary>{t("resources.layers")}</summary>
      <dl className={`${styles.kv} ${memory.values}`}>
        {rows.map(([key, value]) => (
          <Fragment key={key}>
            <dt>{t(`resources.${key}`)}</dt>
            <dd>{value}</dd>
          </Fragment>
        ))}
      </dl>
      <p>{t("resources.separate")}</p>
    </details>
  );
}
