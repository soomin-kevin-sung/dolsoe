import { useCallback, useEffect, useState } from "react";

export type StartPagePreference = "home" | "last-conversation";

export interface GeneralPreferences {
  startPage: StartPagePreference;
  autoLoadLastModel: boolean;
}

const preferencesKey = "local-llm-wiki-general-preferences";
const lastModelPathKey = "local-llm-wiki-last-model-path";
const defaults: GeneralPreferences = {
  startPage: "home",
  autoLoadLastModel: false,
};

function readPreferences(): GeneralPreferences {
  try {
    const value = JSON.parse(localStorage.getItem(preferencesKey) ?? "{}") as Partial<GeneralPreferences>;
    return {
      startPage: value.startPage === "last-conversation" ? "last-conversation" : "home",
      autoLoadLastModel: value.autoLoadLastModel === true,
    };
  } catch {
    return defaults;
  }
}

export function readLastModelPath(): string | null {
  return localStorage.getItem(lastModelPathKey);
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
