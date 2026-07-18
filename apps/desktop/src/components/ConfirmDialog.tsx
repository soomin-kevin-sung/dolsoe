import { useEffect, useRef, type KeyboardEvent, type RefObject } from "react";

type DialogType = "reset" | "reload" | "delete";

function dialogCopy(type: DialogType, messageCount: number) {
  if (type === "reset") return { title: "대화를 초기화할까요?", body: `이 대화의 메시지 ${messageCount}개가 모두 삭제됩니다. 대화와 설정은 유지됩니다. 이 작업은 되돌릴 수 없습니다.`, confirm: "초기화" };
  if (type === "delete") return { title: "대화를 삭제할까요?", body: `메시지 ${messageCount}개가 함께 삭제됩니다. 이 작업은 되돌릴 수 없습니다.`, confirm: "삭제" };
  return { title: "모델을 다시 로드할까요?", body: "진행 중인 생성 1건이 취소되고, 변경한 옵션으로 모델을 다시 로드합니다.", confirm: "다시 로드" };
}

export function ConfirmDialog({ type, messageCount = 4, onClose, onConfirm, returnFocusRef }: { type: DialogType; messageCount?: number; onClose(): void; onConfirm?(): void; returnFocusRef?: RefObject<HTMLButtonElement | null> }) {
  const cancelRef = useRef<HTMLButtonElement>(null); const dialogRef = useRef<HTMLDivElement>(null); const value = dialogCopy(type, messageCount);
  useEffect(() => { cancelRef.current?.focus(); return () => returnFocusRef?.current?.focus(); }, [returnFocusRef]);
  function trapFocus(event: KeyboardEvent<HTMLDivElement>) { if (event.key !== "Tab") return; const buttons = dialogRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"); if (!buttons?.length) return; const first = buttons[0]; const last = buttons[buttons.length - 1]; if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); } }
  return <div className="dialog-scrim"><div ref={dialogRef} role="dialog" aria-modal="true" aria-labelledby="dialog-title" className="confirm-dialog" onKeyDown={trapFocus}><h2 id="dialog-title">{value.title}</h2><p>{value.body}</p><div className="dialog-actions"><button ref={cancelRef} className="button-secondary" type="button" onClick={onClose}>취소</button><button className={type === "reload" ? "button-primary" : "button-danger"} type="button" onClick={() => { onConfirm?.(); onClose(); }}>{value.confirm}</button></div></div></div>;
}
