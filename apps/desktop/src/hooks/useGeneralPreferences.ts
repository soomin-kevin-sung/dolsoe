import { useCallback, useEffect, useState } from "react";

export type StartPagePreference = "home" | "last-conversation";

export interface GeneralPreferences {
  startPage: StartPagePreference;
  autoLoadLastModel: boolean;
}

const preferencesKey = "dolsoe.general-preferences";
const legacyPreferencesKey = "local-llm-wiki-general-preferences";
const lastModelPathKey = "dolsoe.last-model-path";
const legacyLastModelPathKey = "local-llm-wiki-last-model-path";
const defaults: GeneralPreferences = {
  startPage: "home",
  autoLoadLastModel: false,
};

function readPreferences(): GeneralPreferences {
  try {
    const stored = localStorage.getItem(preferencesKey) ?? localStorage.getItem(legacyPreferencesKey);
    if (stored && !localStorage.getItem(preferencesKey)) {
      localStorage.setItem(preferencesKey, stored);
    }
    const value = JSON.parse(stored ?? "{}") as Partial<GeneralPreferences>;
    return {
      startPage: value.startPage === "last-conversation" ? "last-conversation" : "home",
      autoLoadLastModel: value.autoLoadLastModel === true,
    };
  } catch {
    return defaults;
  }
}

export function readLastModelPath(): string | null {
  const stored = localStorage.getItem(lastModelPathKey) ?? localStorage.getItem(legacyLastModelPathKey);
  if (stored && !localStorage.getItem(lastModelPathKey)) {
    localStorage.setItem(lastModelPathKey, stored);
  }
  return stored;
}

export function rememberLastModelPath(modelPath: string): void {
  localStorage.setItem(lastModelPathKey, modelPath);
}

export function useGeneralPreferences() {
  const [preferences, setPreferences] = useState<GeneralPreferences>(readPreferences);

  useEffect(() => {
    localStorage.setItem(preferencesKey, JSON.stringify(preferences));
  }, [preferences]);

  const updatePreferences = useCallback((next: Partial<GeneralPreferences>) => {
    setPreferences((current) => ({ ...current, ...next }));
  }, []);

  return [preferences, updatePreferences] as const;
}
