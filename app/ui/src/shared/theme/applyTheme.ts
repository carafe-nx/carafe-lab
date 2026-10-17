const darkQuery = "(prefers-color-scheme: dark)";

export function followSystemTheme(): () => void {
  const media = window.matchMedia(darkQuery);
  const apply = () => document.documentElement.setAttribute("data-theme", media.matches ? "dark" : "light");
  apply();
  media.addEventListener("change", apply);
  return () => media.removeEventListener("change", apply);
}
