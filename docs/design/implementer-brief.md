# 구현자(Codex) 전달용 프롬프트 — UI 구현

이 문서는 승인된 디자인 산출물을 기준으로 프런트엔드를 구현할 때 구현자에게 그대로 전달한다.
디자인 의뢰서(`claude-design-brief.md`)와 짝을 이루는 구현 의뢰서다.

## 구현자에게 전달할 요청

```text
당신은 Tauri 2 + React + TypeScript 데스크톱 앱의 시니어 프런트엔드 엔지니어다.

먼저 다음 문서를 순서대로 읽어라. 디자인 결정은 이미 끝났으며 재해석하지 않는다.
1. docs/design/claude-ui-spec.md            — 구현 기준 명세 (토큰·치수·상태·상호작용 전부)
2. docs/design/mockups/mockup.html          — 살아 있는 목업. 스펙과 수치가 1:1이며,
   ?state=<상태>&theme=<light|dark> 로 14개 상태를 재현할 수 있다
3. docs/design/mockups/*.png                — 상태별 기대 결과 (2배 해상도)
4. docs/superpowers/specs/2026-07-18-local-llm-desktop-mvp-design.md — 제품 범위와 아키텍처

목표:
apps/desktop 의 React UI를 claude-ui-spec.md 대로 구현한다. 이번 범위는 UI 계층이다.
네이티브 추론이 아직 없으므로 Tauri command/event 를 감싸는 서비스 인터페이스를 정의하고,
mock 구현으로 14개 상태를 모두 재현할 수 있게 한다 (MVP 설계 §14: mock Tauri API + Playwright).

작업 순서:
1. src/styles/tokens.css — 스펙 §2 의 CSS 커스텀 프로퍼티를 그대로 정의한다.
   컴포넌트는 토큰만 참조하고 hex 를 직접 쓰지 않는다.
2. 스펙 §8.1 의 컴포넌트 경계대로 구현한다: Sidebar, SessionItem, ChatHeader, ModelChip,
   MessageList, UserMessage, AssistantMessage, MetricsLine, ErrorBlock, Composer, StatusBar,
   SettingsPanel, SegmentedControl, OptionRow, PackRow, DiagnosticsView, ConfirmDialog.
3. 앱 상태 머신: 모델 없음 → 로딩 중 → 준비됨 ⇄ 생성 중 → (완료|취소|오류) + 스펙 §4 의
   14개 상태를 mock 서비스로 전환 가능하게 한다.
4. tauri.conf.json 창 설정을 스펙 §8.3 값(1440×900, min 1024×700, title "Local LLM Wiki")으로
   교체하고, 앱 아이콘을 §8.4 명령으로 생성한다:
   cd apps/desktop && npm run tauri icon ../../design/app-icon-1024.png
5. Playwright 로 상태별 화면을 검증한다 (아래 합격 기준).

반드시 지켜라:
- 모든 사용자 문구는 mockup.html 의 한국어 카피를 글자 그대로 사용한다. 새로 쓰지 않는다.
- 아이콘은 lucide-react, 이름은 스펙 §8.2 표를 따른다.
- 수치(tok/s, 토큰, 컨텍스트)는 --font-mono + font-variant-numeric: tabular-nums.
- 키보드/포커스/Esc 우선순위는 스펙 §6, 반응형은 §5 (1280/1180 브레이크포인트).
- 다크 모드는 documentElement 의 data-theme 토글 + 설정 패널 "화면" 섹션 연동.
- prefers-reduced-motion 에서 커서·펄스·스피너·패널 트랜지션을 끈다.
- 스트리밍 텍스트에 overflow-wrap: break-word / word-break: break-word / white-space: pre-wrap.

합격 기준:
- 14개 상태가 mock 전환으로 재현되고, 각각 docs/design/mockups/ 의 대응 PNG와 시각적으로
  일치한다 (레이아웃·색·문구·상태 표시 기준, 픽셀 퍼펙트 요구 아님).
- 1024×700 최소 창에서 겹침·잘림·가로 스크롤이 없다.
- 라이트/다크 모두에서 위 조건을 만족한다.
- 긴 모델 파일명(예: 80자)과 공백 없는 긴 토큰이 칩·상태바·메시지에서 레이아웃을 깨지 않는다.
- 키보드만으로 새 대화 → 입력 → 전송 → 중지 → 설정 열기/닫기가 가능하고 포커스 링이 보인다.

하지 마라:
- 디자인 값 변경(색·치수·radius·문구). 모호하면 mockup.html 의 CSS/JS 가 정답이다.
- 스펙에 없는 기능 추가 (YAGNI).
- 네이티브 추론/DLL 연동 — 이번 범위 밖이며 서비스 인터페이스 뒤에 남겨 둔다.

끝나면 보고하라:
- 생성·수정한 파일 목록
- 상태별 재현 방법 (mock 전환 방법)
- 스펙과 다르게 구현할 수밖에 없었던 지점과 이유
- 스펙 §9 미결정 사항 중 구현 중 결정이 필요했던 항목
```

## 사용 방법

1. 위 블록 전체를 구현자에게 전달한다.
2. 구현자가 보고한 "스펙과 다른 지점"은 디자인 스펙을 갱신하거나 구현을 수정해 정합을 유지한다.
3. UI 승인 후 네이티브 연동(Tauri command ↔ llm-runtime)은 별도 계획으로 진행한다.
