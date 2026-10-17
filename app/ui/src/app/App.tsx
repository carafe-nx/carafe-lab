import { useEffect } from "react";
import { RouterProvider } from "react-router";
import { LabProvider } from "@/features/lab/LabProvider";
import { followSystemTheme } from "@/shared/theme/applyTheme";
import { router } from "./router";

export function App() {
  useEffect(() => followSystemTheme(), []);
  return (
    <LabProvider>
      <RouterProvider router={router} />
    </LabProvider>
  );
}
