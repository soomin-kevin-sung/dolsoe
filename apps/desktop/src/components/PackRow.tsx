import type { RuntimePack } from "../services/runtime";

export function PackRow({ pack }: { pack: RuntimePack }) {
  return <div className="pack-row"><div className="pack-heading"><strong>{pack.name} 런타임 팩</strong><span className="mono">118 MB</span>{pack.status === "available" && <button className="button-secondary" type="button">설치</button>}{pack.status === "installing" && <span className="reload-badge">설치 중</span>}</div>{pack.status === "installing" && <><div className="progress-track"><div className="progress-fill" style={{ width: `${pack.progress ?? 0}%` }} /></div><div className="pack-status">다운로드 중 | {pack.progress}% · 75.5 / 118 MB</div></>}<p>서명과 체크섬을 검증한 뒤 다음 시작 시 활성화됩니다. 창을 닫아도 설치는 계속됩니다.</p></div>;
}
