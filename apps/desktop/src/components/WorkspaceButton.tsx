import { Folder } from "lucide-react";

import { workspaceDisplayName } from "../services/workspacePaths";

interface Props {
  path: string;
  disabled?: boolean;
  onChange(): void;
}

export function WorkspaceButton({ path, disabled = false, onChange }: Props) {
  if (!path) return null;
  const name = workspaceDisplayName(path);
  return (
    <button
      type="button"
      className="workspace-trigger"
      disabled={disabled}
      title={`작업 폴더: ${path}`}
      aria-label={`작업 폴더 ${name} 변경`}
      onClick={onChange}
    >
      <Folder size={14} strokeWidth={1.8} aria-hidden="true" />
      <span>{name}</span>
    </button>
  );
}
