import type { AutorunSettings } from "@/shared/api/bindings/AutorunSettings";

export function settingsSummary(settings: AutorunSettings): string[] {
  const graphics = settings.graphics;
  const parts = [
    graphics.direct3d,
    `dxvk ${graphics.dxvkSource}`,
    settings.system.cpu,
    settings.input.mode === "keyboardMouse" ? "keyboard" : "controller",
  ];
  if (graphics.hud !== "off") parts.push(`hud ${graphics.hud}`);
  if (graphics.frameLimit > 0) parts.push(`${graphics.frameLimit} fps`);
  return parts;
}
