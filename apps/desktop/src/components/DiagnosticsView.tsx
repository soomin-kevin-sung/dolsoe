const sections = [
  ["앱", [["앱 버전", "0.1.0"], ["Bridge ABI", "1.2"], ["llama.cpp", "6bdd77f"]]],
  ["런타임 팩", [["활성 버전", "2026.07.1 · CPU · CUDA"], ["검증 상태", "서명 · SHA-256 통과"], ["롤백 가능 버전", "2026.06.3"]]],
  ["장치", [["백엔드", "CUDA"], ["장치", "NVIDIA GeForce RTX 4070 · 12 GB"], ["드라이버", "566.14 · CUDA 12.6"]]],
  ["모델", [["파일", "Qwen2.5-7B-Instruct-Q4_K_M.gguf"], ["아키텍처", "llama · Q4_K_M"], ["로딩 시간", "6.2s"]]],
  ["최근 추론", [["프롬프트", "214 토큰 · 1,542 tok/s"], ["생성", "164 토큰 · 42.1 tok/s"], ["첫 토큰 시간", "0.42s"], ["전체 시간", "3.9s · 완료"]]],
  ["자원 · 스케줄러", [["RAM / VRAM", "2.1 GB / 6.8 GB"], ["컨텍스트", "1,847 / 8,192"], ["활성 슬롯", "1 / 2 · 큐 대기 0"], ["취소 지연", "평균 18 ms"]]],
] as const;
export function DiagnosticsView() { return <div className="diagnostics"><h1>진단</h1>{sections.map(([title, rows]) => <section className="diagnostic-section" key={title}><h2>{title}</h2>{rows.map(([label, value]) => <div className="diagnostic-row" key={label}><span>{label}</span><code>{value}</code></div>)}</section>)}</div>; }
