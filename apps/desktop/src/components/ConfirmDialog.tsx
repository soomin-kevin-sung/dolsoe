import { useEffect, useRef, type RefObject } from "react";

const copy = {
  reset: { title: "대화를 초기화할까요?", body: "이 대화의 메시지 4개가 모두 삭제됩니다. 대화와 설정은 유지됩니다. 이 작업은 되돌릴 수 없습니다.", confirm: "초기화" },
  reload: { title: "모델을 다시 로드할까요?", body: "진행 중인 생성 1건이 취소되고, 변경한 옵션으로 모델을 다시 로드합니다.", confirm: "다시 로드" },
} as const;

export function ConfirmDialog({ type, onClose, returnFocusRef }: { type: keyof typeof copy; onClose(): void; returnFocusRef: RefObject<HTMLButtonElement | null> }) {
  const cancelRef = useRef<HTMLButtonElement>(null); const value = copy[type];
  useEffect(() => { cancelRef.current?.focus(); return () => returnFocusRef.current?.focus(); }, [returnFocusRef]);
  return <div className="dialog-scrim"><div role="dialog" aria-modal="true" aria-labelledby="dialog-title" className="confirm-dialog"><h2 id="dialog-title">{value.title}</h2><p>{value.body}</p><div className="dialog-actions"><button ref={cancelRef} className="button-secondary" type="button" onClick={onClose}>취소</button><button className={type === "reset" ? "button-danger" : "button-primary"} type="button" onClick={onClose}>{value.confirm}</button></div></div></div>;
}
