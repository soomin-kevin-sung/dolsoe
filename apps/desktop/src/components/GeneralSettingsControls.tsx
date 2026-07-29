import { Folder, FolderOpen } from "lucide-react";

import type { StartPagePreference } from "../hooks/useGeneralPreferences";
import type { ThemePreference } from "../services/runtime";
import { SegmentedControl } from "./SegmentedControl";

interface Props {
  theme: ThemePreference;
  startPage: StartPagePreference;
  autoLoadLastModel: boolean;
  defaultWorkspacePath?: string;
  onThemeChange(theme: ThemePreference): void;
  onStartPageChange(startPage: StartPagePreference): void;
  onAutoLoadLastModelChange(enabled: boolean): void;
  onDefaultWorkspaceChange?(): void;
}

export function GeneralSettingsControls(props: Props) {
  return (
    <>
      <section className="settings-section settings-section-first">
        <h3>화면</h3>
        <div className="general-setting-row">
          <div className="general-setting-copy">
            <strong>테마</strong>
            <small>앱 전체의 밝기와 대비를 선택합니다.</small>
          </div>
          <div className="general-setting-control">
            <SegmentedControl label="테마" value={props.theme} onChange={props.onThemeChange} items={[{ value: "light", label: "라이트" }, { value: "dark", label: "다크" }, { value: "system", label: "시스템" }]} />
          </div>
        </div>
      </section>
      <section className="settings-section">
        <h3>시작</h3>
        <div className="general-setting-row">
          <div className="general-setting-copy">
            <strong>시작 화면</strong>
            <small>앱을 열었을 때 처음 표시할 화면입니다.</small>
          </div>
          <div className="general-setting-control">
            <SegmentedControl<StartPagePreference> label="시작 화면" value={props.startPage} onChange={props.onStartPageChange} items={[{ value: "home", label: "홈" }, { value: "last-conversation", label: "마지막 대화" }]} />
          </div>
        </div>
        <div className="general-setting-row">
          <div className="general-setting-copy">
            <strong>마지막 모델 자동 로드</strong>
            <small>마지막으로 사용한 모델을 다음 앱 시작 시 자동으로 불러옵니다.</small>
          </div>
          <label className="switch-control">
            <input type="checkbox" aria-label="마지막 모델 자동 로드" checked={props.autoLoadLastModel} onChange={(event) => props.onAutoLoadLastModelChange(event.target.checked)} />
            <span aria-hidden="true" />
            <strong>{props.autoLoadLastModel ? "사용" : "사용 안 함"}</strong>
          </label>
        </div>
      </section>
      {props.defaultWorkspacePath && props.onDefaultWorkspaceChange && (
        <section className="settings-section">
          <h3>작업 폴더</h3>
          <div className="general-setting-row">
            <div className="general-setting-copy">
              <strong>새 대화 기본 폴더</strong>
              <small>새 대화마다 독립적으로 저장됩니다.</small>
            </div>
            <div className="workspace-path-control">
              <div className="workspace-path" title={props.defaultWorkspacePath}>
                <Folder size={14} strokeWidth={1.8} aria-hidden="true" />
                <span>{props.defaultWorkspacePath}</span>
              </div>
              <button
                className="button-secondary"
                type="button"
                onClick={props.onDefaultWorkspaceChange}
              >
                <FolderOpen size={14} aria-hidden="true" />
                변경
              </button>
            </div>
          </div>
        </section>
      )}
    </>
  );
}
