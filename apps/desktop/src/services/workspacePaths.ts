import { open } from "@tauri-apps/plugin-dialog";

export async function chooseWorkspaceDirectory(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: true,
  });
  return typeof selected === "string" ? selected : null;
}

export function workspaceDisplayName(path: string): string {
  const normalized = path.replace(/[\\/]+$/, "");
  const segments = normalized.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? path;
}
