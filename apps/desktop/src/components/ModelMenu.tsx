import { Cpu, Layers3 } from "lucide-react";
import { useEffect, useState } from "react";

interface Props {
  open: boolean;
  modelName: string;
  modelPath: string | null;
  runtimeLabel: string;
  backend: string;
  actionsDisabled?: boolean;
  onChooseModel(): Promise<string | null>;
  onReplace(modelPath: string): Promise<boolean>;
  onUnload(): void;
  onOpenRuntime(): void;
  onClose(): void;
}

function fileName(modelPath: string): string {
  const parts = modelPath.split(/[\\/]/);
  return parts[parts.length - 1] ?? modelPath;
}

function quantization(modelName: string): string | null {
  return modelName.match(/Q\d(?:_[A-Z0-9]+)+/i)?.[0] ?? null;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function ModelMenu(props: Props) {
  const [draftPath, setDraftPath] = useState<string | null>(null);
  const [baseModelName, setBaseModelName] = useState(props.modelName);
  const [baseModelPath, setBaseModelPath] = useState(props.modelPath);
  const [choosing, setChoosing] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const draftName = draftPath ? fileName(draftPath) : null;
  const currentQuantization = quantization(props.modelName);

  useEffect(() => {
    if (!props.open) return;
    setBaseModelName(props.modelName);
    setBaseModelPath(props.modelPath);
    setDraftPath(null);
    setChoosing(false);
    setApplying(false);
    setError(null);
  }, [props.open]);

  async function chooseModel() {
    setChoosing(true);
    setError(null);
    try {
      const selectedPath = await props.onChooseModel();
      if (selectedPath && selectedPath !== baseModelPath) setDraftPath(selectedPath);
    } catch (reason) {
      setError(`모델 파일을 선택하지 못했습니다. ${errorText(reason)}`);
    } finally {
      setChoosing(false);
    }
  }

  async function replaceModel() {
    if (!draftPath) return;
    setApplying(true);
    setError(null);
    try {
      const applied = await props.onReplace(draftPath);
      if (applied) props.onClose();
      else setError(baseModelPath
        ? "새 모델을 불러오지 못해 기존 모델로 복구했습니다."
        : "모델을 불러오지 못했습니다.");
    } catch (reason) {
      setError(`모델을 불러오지 못했습니다. ${errorText(reason)}`);
    } finally {
      setApplying(false);
    }
  }

  return (
    <div id="model-management-menu" className="model-menu" role="dialog" aria-label="모델 관리">
      {draftName ? (
        <>
          <div className="model-menu-heading">
            <strong>모델 교체</strong>
            <span>선택만으로는 현재 모델이 바뀌지 않습니다.</span>
          </div>
          <div className="model-menu-compare">
            <div>
              <span>현재</span>
              <strong>{baseModelName || "모델 없음"}</strong>
            </div>
            <div className="selected">
              <span>선택</span>
              <strong>{draftName}</strong>
            </div>
          </div>
          {error && <p className="model-menu-error" role="alert">{error}</p>}
          <div className="model-menu-actions">
            <button className="button-secondary" type="button" disabled={applying} onClick={() => { setDraftPath(null); setError(null); }}>취소</button>
            <button className="button-primary" type="button" disabled={props.actionsDisabled || applying} onClick={() => void replaceModel()}>
              {applying ? "모델 교체 중..." : baseModelPath ? "이 모델로 교체" : "모델 로드"}
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="model-menu-heading-row">
            <span className="model-menu-icon" aria-hidden="true"><Layers3 size={17} /></span>
            <div className="model-menu-heading">
              <span>{props.modelName ? "현재 모델" : "모델"}</span>
              <strong>{props.modelName || "선택된 모델 없음"}</strong>
            </div>
            <span className="model-menu-status"><i aria-hidden="true" />{props.runtimeLabel}</span>
          </div>
          <p className="model-menu-meta">{props.backend}{currentQuantization ? ` · ${currentQuantization}` : ""}</p>
          {error && <p className="model-menu-error" role="alert">{error}</p>}
          <div className="model-menu-actions">
            <button className="button-primary" type="button" disabled={props.actionsDisabled || choosing} onClick={() => void chooseModel()}>
              {choosing ? "선택하는 중..." : props.modelName ? "다른 모델 선택" : "모델 선택"}
            </button>
            {props.modelName && <button className="button-secondary" type="button" disabled={props.actionsDisabled} onClick={props.onUnload}>언로드</button>}
          </div>
          <button className="model-menu-runtime" type="button" onClick={props.onOpenRuntime}>
            <span><Cpu size={14} aria-hidden="true" />런타임 설정</span>
            <span aria-hidden="true">›</span>
          </button>
        </>
      )}
    </div>
  );
}
