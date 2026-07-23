import type { RuntimePack } from "../services/runtime";

export function PackRow({ pack, installBusy = false }: { pack: RuntimePack; installBusy?: boolean }) {
  return <div className="pack-row"><div className="pack-heading"><strong>{pack.name} 런타임 팩</strong><span className="mono">18 MB</span>{pack.status === "available" && <button className="button-secondary" type="button" disabled={installBusy}>설치</button>}{pack.status === "installing" && <span className="reload-badge">설치 중</span>}</div>{pack.status === "installing" && <><div className="progress-track"><div className="progress-fill" style={{ width: `${pack.progress ?? 0}%` }} /></div><div className="pack-status">다운로드 중 | {pack.progress}% · 12 / 18 MB</div></>}<p>매니페스트와 파일 체크섬을 확인한 뒤 다음 시작 시 활성화됩니다. 창을 닫아도 설치는 계속됩니다.</p></div>;
}
