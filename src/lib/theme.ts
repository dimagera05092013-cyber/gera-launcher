import {
  Hct,
  MaterialDynamicColors,
  SchemeTonalSpot,
  argbFromHex,
  hexFromArgb,
} from "@material/material-color-utilities";

type Mode = "light" | "heavy";

type Role = (scheme: SchemeTonalSpot) => number;

const roles: Array<[string, Role]> = [
  ["primary", (s) => s.primary],
  ["on-primary", (s) => s.onPrimary],
  ["primary-container", (s) => s.primaryContainer],
  ["on-primary-container", (s) => s.onPrimaryContainer],
  ["primary-fixed", (s) => MaterialDynamicColors.primaryFixed.getArgb(s)],
  ["primary-fixed-dim", (s) => MaterialDynamicColors.primaryFixedDim.getArgb(s)],
  ["on-primary-fixed", (s) => MaterialDynamicColors.onPrimaryFixed.getArgb(s)],
  ["on-primary-fixed-variant", (s) => MaterialDynamicColors.onPrimaryFixedVariant.getArgb(s)],
  ["secondary", (s) => s.secondary],
  ["on-secondary", (s) => s.onSecondary],
  ["secondary-container", (s) => s.secondaryContainer],
  ["on-secondary-container", (s) => s.onSecondaryContainer],
  ["secondary-fixed", (s) => MaterialDynamicColors.secondaryFixed.getArgb(s)],
  ["on-secondary-fixed", (s) => MaterialDynamicColors.onSecondaryFixed.getArgb(s)],
  ["tertiary", (s) => s.tertiary],
  ["on-tertiary", (s) => s.onTertiary],
  ["tertiary-container", (s) => s.tertiaryContainer],
  ["on-tertiary-container", (s) => s.onTertiaryContainer],
  ["tertiary-fixed", (s) => MaterialDynamicColors.tertiaryFixed.getArgb(s)],
  ["on-tertiary-fixed", (s) => MaterialDynamicColors.onTertiaryFixed.getArgb(s)],
  ["error", (s) => s.error],
  ["on-error", (s) => s.onError],
  ["error-container", (s) => s.errorContainer],
  ["on-error-container", (s) => s.onErrorContainer],
  ["background", (s) => s.background],
  ["on-background", (s) => s.onBackground],
  ["surface", (s) => s.surface],
  ["on-surface", (s) => s.onSurface],
  ["surface-variant", (s) => s.surfaceVariant],
  ["on-surface-variant", (s) => s.onSurfaceVariant],
  ["surface-dim", (s) => s.surfaceDim],
  ["surface-bright", (s) => s.surfaceBright],
  ["surface-container-lowest", (s) => s.surfaceContainerLowest],
  ["surface-container-low", (s) => s.surfaceContainerLow],
  ["surface-container", (s) => s.surfaceContainer],
  ["surface-container-high", (s) => s.surfaceContainerHigh],
  ["surface-container-highest", (s) => s.surfaceContainerHighest],
  ["surface-tint", (s) => s.surfaceTint],
  ["outline", (s) => s.outline],
  ["outline-variant", (s) => s.outlineVariant],
  ["inverse-surface", (s) => s.inverseSurface],
  ["inverse-on-surface", (s) => s.inverseOnSurface],
  ["inverse-primary", (s) => s.inversePrimary],
  ["scrim", (s) => s.scrim],
];

let media: MediaQueryList | null = null;

export function resolveDark(theme: "system" | "light" | "dark"): boolean {
  if (theme === "light") return false;
  if (theme === "dark") return true;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function applyTheme(
  accent: string,
  theme: "system" | "light" | "dark",
  uiMode: Mode,
  gradients: boolean,
): boolean {
  const isDark = resolveDark(theme);
  let scheme: SchemeTonalSpot;
  try {
    scheme = new SchemeTonalSpot(Hct.fromInt(argbFromHex(accent)), isDark, 0);
  } catch {
    scheme = new SchemeTonalSpot(Hct.fromInt(argbFromHex("#6750A4")), isDark, 0);
  }
  const root = document.documentElement;
  for (const [name, pick] of roles) {
    root.style.setProperty(`--${name}`, hexFromArgb(pick(scheme)));
  }
  root.dataset.theme = isDark ? "dark" : "light";
  root.dataset.ui = uiMode;
  root.dataset.gradient = gradients ? "on" : "off";
  root.style.colorScheme = isDark ? "dark" : "light";
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", hexFromArgb(scheme.surface));

  if (!media) {
    media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", () => {
      const stored = localStorage.getItem("gera.theme");
      const storedAccent = localStorage.getItem("gera.accent");
      const storedUi = localStorage.getItem("gera.ui");
      const storedGradients = localStorage.getItem("gera.gradients");
      if (stored) {
        applyTheme(
          storedAccent || "#6750A4",
          stored as "system" | "light" | "dark",
          (storedUi as Mode) || "light",
          storedGradients !== "off",
        );
      }
    });
  }
  localStorage.setItem("gera.theme", theme);
  localStorage.setItem("gera.accent", accent);
  localStorage.setItem("gera.ui", uiMode);
  localStorage.setItem("gera.gradients", gradients ? "on" : "off");
  return isDark;
}
