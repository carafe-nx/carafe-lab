import { createContext, type ReactNode, useContext, useMemo, useState } from "react";
import type { LabStatus } from "@/shared/api/bindings/LabStatus";
import type { LabView } from "@/shared/api/bindings/LabView";
import type { RunExit } from "@/shared/api/bindings/RunExit";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { api } from "@/shared/api/commands";
import { type Polled, usePolling } from "@/shared/api/usePolling";
import { useTauriEvent } from "@/shared/api/useTauriEvent";

type LabContextValue = {
  status: Polled<LabStatus>;
  lab: Polled<LabView>;
  runs: Polled<RunSummary[]>;
  exit: RunExit | null;
  dismissExit: () => void;
};

const LabContext = createContext<LabContextValue | null>(null);

export function LabProvider({ children }: { children: ReactNode }) {
  const status = usePolling(api.getStatus, 2000);
  const lab = usePolling(api.getLab, 10000);
  const runs = usePolling(api.listRuns, 4000);
  const [exit, setExit] = useState<RunExit | null>(null);
  useTauriEvent("run://exited", setExit);
  const value = useMemo(
    () => ({ status, lab, runs, exit, dismissExit: () => setExit(null) }),
    [status, lab, runs, exit],
  );
  return <LabContext.Provider value={value}>{children}</LabContext.Provider>;
}

export function useLab(): LabContextValue {
  const value = useContext(LabContext);
  if (!value) {
    throw new Error("useLab outside LabProvider");
  }
  return value;
}
