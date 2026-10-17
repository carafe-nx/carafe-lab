import { createHashRouter, Navigate } from "react-router";
import { ComparePage } from "@/pages/compare/ComparePage";
import { EmulatorPage } from "@/pages/emulator/EmulatorPage";
import { HistoryPage } from "@/pages/history/HistoryPage";
import { LibraryPage } from "@/pages/library/LibraryPage";
import { PlansPage } from "@/pages/plans/PlansPage";
import { RunPage } from "@/pages/run/RunPage";
import { ScenariosPage } from "@/pages/scenarios/ScenariosPage";
import { VersionsPage } from "@/pages/versions/VersionsPage";
import { Shell } from "./Shell";

export const router = createHashRouter([
  {
    path: "/",
    element: <Shell />,
    children: [
      { index: true, element: <Navigate to="/library" replace /> },
      { path: "library", element: <LibraryPage /> },
      { path: "plans", element: <PlansPage /> },
      { path: "run", element: <RunPage /> },
      { path: "history", element: <HistoryPage /> },
      { path: "history/:dir", element: <HistoryPage /> },
      { path: "compare/:a/:b", element: <ComparePage /> },
      { path: "versions", element: <VersionsPage /> },
      { path: "emulator", element: <EmulatorPage /> },
      { path: "scenarios", element: <ScenariosPage /> },
    ],
  },
]);
