import { useEffect, useState } from "react";

import type { ThemePreference } from "../services/runtime";

const storageKey = "dolsoe.theme";

function storedPreference(): ThemePreference {
  const value = localStorage.getItem(storageKey);
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

function resolvedTheme(preference: ThemePreference): "light" | "dark" {
  if (preference !== "system") return preference;
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function useThemePreference(initialPreference?: ThemePreference) {
  const [preference, setPreference] = useState<ThemePreference>(() => initialPreference ?? storedPreference());

  useEffect(() => {
    localStorage.setItem(storageKey, preference);
    document.documentElement.dataset.theme = resolvedTheme(preference);

    if (preference !== "system") return;
    const media = matchMedia("(prefers-color-scheme: dark)");
    const update = () => { document.documentElement.dataset.theme = resolvedTheme("system"); };
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [preference]);

  return [preference, setPreference] as const;
}
