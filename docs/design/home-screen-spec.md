# 돌쇠 — 홈 화면 설계 명세

작성일: 2026-07-21 · 대상: `apps/desktop` (Tauri 2 + React + TypeScript)
기준 창 크기: 기본 **1180×660**, 최소 **900×580** (`apps/desktop/src-tauri/tauri.conf.json` 실측값).
스타일·표기 관례는 `docs/design/claude-ui-spec.md`를 따른다. 다만 그 문서의 창 크기(1440×900 / 1024×700)는
구버전 수치이므로 이 문서에서는 참조하지 않는다.

런타임 적용 정책: 최초 CPU 설치와 비활성 백엔드 설치는 현재 프로세스에서 즉시 사용할 수 있다. 이 문서의
재시작 상태는 `installState.restartRequired === true` 또는 `replacement-pending`인 활성 DLL 교체에만 적용한다.

살아 있는 목업은 `docs/design/mockups/home-mockup.html`이며, `?state=<상태>&theme=<light|dark>`로
이 문서의 모든 상태를 재현한다(11절 매핑표 참조). 토큰 값은 이 문서와 목업 모두
`apps/desktop/src/styles/tokens.css`를 그대로 따른다 — 이 문서 본문에는 hex를 직접 쓰지 않는다.

---

## 1. 개요와 설계 원칙

현재 앱은 실행 즉시 채팅 화면으로 들어간다. 그러나 실제로 대화를 시작하려면 (1) CPU 런타임이 설치되어 있고,
(2) GGUF 모델이 로드되어 있어야 한다. 이 두 준비 단계가 채팅 화면 안에서 빈 상태·오류 블록으로만 드러나기 때문에
사용자는 "지금 무엇을 해야 앱을 쓸 수 있는지"를 스스로 추론해야 했다. 홈 화면은 이 준비 순서를
**CPU 런타임 → 모델 → 대화** 라는 하나의 축으로 정리해 보여주고, 그 순간 가장 먼저 해야 할 행동 하나를
명확히 강조하는 착지 화면이다.

설계 원칙:

1. **조용한 도구 UI.** 마케팅 랜딩 페이지나 온보딩 마법사가 아니다. 히어로 배너, 중첩 카드, 그라디언트,
   일러스트, 장황한 사용법 설명을 두지 않는다. 기존 사이드바·헤더·설정 패널과 같은 시각 언어
   (뉴트럴이 넓은 면적을 차지하고, teal은 정상·주 동작, amber는 재시작 필요, red는 실제 실패)를 그대로 잇는다.
2. **주 행동은 언제나 하나.** 화면에는 그 순간 사용자가 해야 할 primary 버튼이 최대 1개만 존재한다.
   진행 중이라 결정할 것이 없는 상태(다운로드/검증/설치/모델 로딩 중)는 primary 버튼이 0개일 수 있다 —
   "정확히 1개"라는 규칙은 "서로 경쟁하는 여러 primary 버튼을 두지 않는다"는 뜻으로 적용한다.
3. **순서는 구조로, 설명으로 말하지 않는다.** 화면 상단의 준비 단계 표시줄(4.3절, 5절 각 상태)이
   CPU 런타임 → 모델 → 대화 중 지금 어디에 있는지 항상 보여준다. 번호 매김 스테퍼나 마법사 진행 바처럼
   과장하지 않고, 상태바와 같은 절제된 텍스트+점 표기를 쓴다.
4. **기존 컴포넌트를 최대한 재사용한다.** 새 시각 언어를 만들지 않고 `.button-primary`, `.error-block`,
   `.runtime-restart-notice`, `.loading-card`, `.session-item` 등 이미 검증된 클래스를 그대로 쓴다
   (9절 컴포넌트 목록 참조). 이 재사용 우선 원칙에는 브리프의 "버튼에는 Lucide 아이콘 사용" 지침에 대한
   의도적 예외가 하나 있다 — 상태 4(`.runtime-restart-notice`)의 `지금 재시작`/`나중에`와 상태 7
   (`.loading-card`)의 `로드 취소`는 기존 설정 패널·채팅 화면에 이미 있는 버튼을 그대로 재사용하며,
   원본에 아이콘이 없으므로 홈에서도 아이콘을 붙이지 않는다(5.4·5.7절에서 재확인). 그 외 신규로 배치하는
   primary 버튼(상태 1/5/6/8)은 모두 Lucide 아이콘을 붙인다.
5. **CUDA/Vulkan은 홈에서 경쟁하지 않는다.** 홈의 주 행동은 오직 CPU 런타임·모델·대화 세 가지 준비
   단계에만 관여한다. GPU 백엔드 설치는 헤더의 설정 아이콘(항상 노출)을 거쳐 설정 패널에서만 이뤄진다.

---

## 2. 홈 화면 정보 구조

홈은 사이드바·상태바·설정 패널은 그대로 둔 채 중앙 영역(진단 화면과 같은 자리)만 교체하는 뷰다.
중앙 영역 안에서 위에서 아래로 다음 3개 구역이 이 순서로만 존재한다 — 순서 자체가 "먼저 준비 상태를
확인하고, 그다음 지금 쓰는 모델을 확인하고, 마지막으로 예전 대화로 이어간다"는 우선순위를 뜻한다.

| 우선순위 | 구역 | 역할 | 항상 보이는가 |
|---|---|---|---|
| 1 | **준비 상태 패널** (4.3절 `.home-panel` 등) | 지금 막힌 지점과 그 지점의 유일한 primary 행동을 보여준다. CPU 런타임 → 모델 → 대화 진행 표시줄을 내부 상단에 포함한다. | 항상 — 8개 상태 중 정확히 하나가 렌더링된다 |
| 2 | **모델·백엔드 요약 줄** | 현재 로드된 모델명과 활성 백엔드를 상태바와 동일한 어휘(상태점+텍스트)로 한 줄 요약한다. | 항상 |
| 3 | **최근 대화** | 최근 대화 3~5개를 사이드바와 같은 항목 스타일로 보여주고 클릭 시 바로 그 대화로 이동한다. 없으면 절제된 한 줄 빈 상태. | 항상 (목록이 비어도 구역 자체는 남는다) |

이 3구역 외에 홈에는 다른 콘텐츠를 두지 않는다. 사용법 설명, 기능 소개, 통계, 배너는 없다.
사이드바(대화 검색·기존 대화 열람), 헤더(모델 칩·설정 토글), 상태바(텔레메트리)는 홈에서도 평소와
동일하게 계속 사용 가능하다 — 이는 브리프의 "CPU 런타임 없음 상태에서도 기존 대화 열람과 설정·진단은
계속 사용할 수 있다"를 홈의 모든 상태로 일반화한 것이다.

---

## 3. 사용자 흐름

### 3.1 콜드 스타트부터 첫 채팅 진입까지

```
앱 실행
  └─ 항상 홈으로 진입 (이전 세션에서 보던 대화로 자동 복귀하지 않는다)
       │
       ├─ CPU 런타임 미준비 → 홈이 설치/복구 행동을 강조 (5.1~5.5절 상태 1~5)
       │     └─ 최초 설치 완료 → worker 즉시 기동 → 모델 선택 상태로 이어짐
       │        (사용 중인 CPU DLL 교체만 재시작 상태로 전환)
       │
       ├─ CPU 런타임 준비됨, 모델 없음 → 홈이 "GGUF 모델 선택" 강조 (상태 6)
       │     └─ 모델 선택 → 로딩 중(상태 7) → 준비됨(상태 8)
       │
       └─ 모델 준비됨 → 홈이 "새 대화 시작" 강조 (상태 8)
             └─ 클릭 → 드래프트(미저장) 대화로 전환 → 첫 메시지 전송 → 실제 대화로 저장 → 채팅 화면
```

앱은 콜드 스타트마다 **항상 홈에서 시작한다** (브리프 흐름 1을 문자 그대로 적용). 직전 세션에 보던
대화가 있어도 자동으로 복귀하지 않는다. 이유는 두 가지다: (a) 브리프가 명시적으로 요구하고,
(b) 매번 준비 상태를 먼저 보여주는 편이 "조용한 도구" 원칙 — 상태를 숨기지 않는다 — 에 맞는다.
런타임이나 모델이 그사이 바뀌었을 가능성(예: 백그라운드에서 설치 완료)도 매번 홈에서 확인시켜 준다.

### 3.2 흐름 1~6 동작 정의

| # | 흐름 | 동작 정의 |
|---|---|---|
| 1 | 콜드 스타트 → 홈 | 위 3.1절. `homeOpen = true`, `diagnosticsOpen = false`, 대화 미선택. |
| 2 | 사이드바 기존 대화 선택 → 채팅 | `workspace.select(id)` 호출, `homeOpen = false`, `diagnosticsOpen = false`. 대기 중이던 드래프트(3.3절)가 있으면 폐기한다. 포커스는 메시지 입력창으로 이동. |
| 3 | 앱 이름/홈 아이콘 → 홈 | 사이드바 헤더의 앱 마크+앱 이름 전체가 하나의 버튼이다(브리프의 "이름이나 아이콘"을 별개의 두 타깃이 아니라 하나의 확장된 버튼으로 해석 — 24px 높이의 좁은 행에 히트 타깃을 둘로 쪼개는 대신 하나로 합쳐 오조작을 줄인다). 클릭 시 `homeOpen = true`, `diagnosticsOpen = false`. 대기 중이던 드래프트가 있으면 폐기한다. `selectedConversationId`는 유지한다(진단 화면과 동일한 관례 — 8.3절). |
| 4 | 새 대화 버튼 → 준비 상태에 맞는 시작 흐름 | CPU와 모델이 준비됐으면 미저장 드래프트를 즉시 열고, 준비되지 않았으면 홈으로 이동해 필요한 행동 하나를 강조한다. 근거는 3.4절. |
| 5 | 빈 대화 미저장 | 3.3절 드래프트 규칙 전체. |
| 6 | 홈은 준비 상태에 따라 주 행동 하나를 강조 | 5절의 8개 상태 중 정확히 하나가 그 순간의 실제 코드 상태로부터 결정된다(10.2절 우선순위 로직). |

### 3.3 드래프트(미저장) 대화 규칙

현재 코드는 `workspace.create()`(`ConversationService.create`) 호출 즉시 빈 대화를 저장하며,
`Ctrl+N`과 사이드바 새 대화 버튼이 이를 직접 호출한다. 브리프는 "빈 대화는 첫 메시지를 보내기 전까지
저장하지 않는다"를 요구하므로 사용자에게 보이는 동작을 다음과 같이 바꾼다. 아래 표는 UX 계약이며,
DB·IPC 계약은 표 다음의 **구현 전제**를 따라 별도로 변경한다.

**드래프트란**: 아직 `ConversationService.create()`가 호출되지 않은, 클라이언트 메모리에만 존재하는
빈 대화 자리다. DB 행이 없고, `id`가 없고, 사이드바에 어떤 항목으로도 나타나지 않는다.

| 전이 | 트리거 | 결과 |
|---|---|---|
| **생성** | (a) 홈 상태 8의 primary 버튼 "새 대화 시작" 클릭, 또는 (b) CPU·모델 준비 완료 상태에서 `Ctrl+N`/사이드바 새 대화 버튼. | `homeOpen = false`. 중앙 영역은 기존 "새 대화를 시작하세요" 빈 상태(`MessageList`의 `empty` 렌더)를 그대로 보여준다. `selectedConversationId = null` — 사이드바에는 어떤 항목도 활성 표시되지 않는다. 컴포저는 활성화되고 포커스를 받는다. |
| **승격(저장)** | 드래프트 상태에서 첫 메시지 전송(`Composer` submit). | 대화와 첫 사용자·어시스턴트 메시지를 **하나의 백엔드 트랜잭션**으로 생성하는 `workspace.startDraft(prompt)`(가칭)를 호출한다. 성공한 시점부터 사이드바에 새 항목이 나타나고 활성 표시되며, 생성 중 스피너가 붙는다. 실패하면 DB에 빈 대화를 남기지 않고 드래프트와 입력 내용을 유지해 재시도할 수 있게 한다. |
| **폐기** | (a) 드래프트 상태에서 사이드바의 다른 대화 선택, (b) 드래프트 상태에서 홈으로 복귀(앱 이름/홈 아이콘), (c) 드래프트 상태에서 진단 화면 진입. | DB에 아무것도 없었으므로 삭제할 대상이 없다 — `selectedConversationId`를 목적지에 맞게 바꾸는 것만으로 폐기가 끝난다. 확인 모달은 띄우지 않는다(잃을 것이 없으므로). 컴포저에 입력해 두었던 텍스트도 함께 사라진다 — 저장 전이므로 별도 임시 저장소를 두지 않는다(단순함 우선: 임시 텍스트 보존은 브리프가 요구하지 않았다). |
| **재진입(멱등)** | 이미 비어 있는 드래프트를 보고 있는 상태에서 `Ctrl+N`/새 대화 버튼을 다시 누름. | 새 드래프트를 쌓지 않고 현재 드래프트를 유지하며 컴포저에 포커스를 돌려준다. 입력 내용이 있으면 새 드래프트로 덮어쓰지 않는다. |

드래프트는 최대 1개만 존재할 수 있다(전역 상태 하나로 충분). 앱이 종료되면 드래프트는 자동으로
사라진다(애초에 저장된 적이 없다).

**구현 전제**:

1. 현재 `workspace.create()` 후 `workspace.submit(prompt)`을 연속 호출하는 방식은 원자적이지 않으므로
   사용하지 않는다. `conversation_start_new_turn(prompt)` 같은 단일 Tauri 명령이 대화 생성과 첫 턴
   저장을 하나의 SQLite 트랜잭션에서 처리해야 한다.
2. `conversation_bootstrap`은 대화가 없을 때 빈 대화를 자동 생성하지 않고 `selected: null`을 반환할 수
   있어야 한다. 마지막 대화 삭제도 빈 대화를 대체 생성하지 않고 빈 목록을 반환해야 한다.
3. 프런트 상태의 `selectedConversationId: null`은 실제 DB에 대응 행이 없는 드래프트 또는 대화가 하나도
   없는 상태를 뜻한다. 백엔드가 임의로 만든 빈 대화를 숨기는 용도로 사용하지 않는다.

### 3.4 흐름 4의 해석 — 새 대화 버튼은 항상 홈을 경유하는가

브리프 원문은 "새 대화 버튼은 홈의 시작 흐름으로 이동한다"이다. 두 가지 해석이 가능하다.

- **안 A — 항상 홈을 경유.** 사이드바 새 대화 버튼과 `Ctrl+N`은 준비 상태와 무관하게 항상 홈으로
  이동한다. 모델이 이미 준비된 경우 홈의 primary 버튼이 곧 "새 대화 시작"이므로, 진입 직후 자동으로
  그 버튼에 포커스가 가 있어(8.2절) 사용자는 Enter/Space 한 번으로 드래프트를 시작할 수 있다.
- **안 B — 준비 완료 시 홈을 건너뛴다.** CPU·모델이 모두 준비된 상태라면 새 대화 버튼이 곧바로
  드래프트 채팅 화면을 연다(홈을 거치지 않음). 준비가 안 된 상태에서만 홈으로 보낸다.

**채택: 안 B.** `Ctrl+N`과 "새 대화"는 반복 사용에서 즉시 새 작업을 시작한다는 관례가 강하다.
CPU·모델이 준비된 사용자에게 매번 홈과 추가 클릭을 요구하지 않는다. 준비되지 않은 경우에는 같은
진입점이 홈으로 이동해 설치 또는 모델 선택을 안내하므로 첫 실행과 복구 흐름도 놓치지 않는다.

분기 로직은 `startConversationFlow(readiness)` 한 곳에 둔다. 사이드바 버튼과 `Ctrl+N`은 모두 이 함수를
호출하고, 홈의 "새 대화 시작"은 준비 완료 상태에서 같은 `openDraft()`를 호출한다. 따라서 UX는 빠르게
유지하면서 상태 판정이 여러 컴포넌트에 중복되는 문제도 피한다.

---

## 4. 홈 레이아웃 공통 규격

### 4.1 구조 개요

홈은 진단 화면과 동일하게 `.conversation-shell`의 헤더(48px)·상태바(28px)를 유지한 채 `<main class="conversation">` 자리만 교체한다. 헤더 제목은 "홈", 모델 칩·초기화·설정 아이콘은 평소와 같은 자리에 남는다(10.3절에서 초기화 아이콘의 예외를 다룬다). 컴포저는 숨긴다.

콘텐츠 폭은 메시지 칼럼과 동일한 규칙을 그대로 재사용한다: `width: min(100%, 760px)`, `margin: 0 auto`,
좌우 패딩 32px(기본 창) / 20px(≤1179px, 즉 최소 창 900px 포함), 상하 패딩 24px.

### 4.2 1180×660 wireframe

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│ 사이드바 264px          │ 헤더 48px: 홈                모델 칩 ▾ │ ⌫ │ ⚙          │  660px
│┌────────────────────────┼─────────────────────────────────────────────────────────┤
││[마크+이름]=홈버튼   [+]│ ← 32px →                                    ← 32px →    │
││ 🔍 대화 검색            │  ┌ 상단 패딩 24px                                        │
││                        │  │ ┌──────────────────────────────────────────┐         │
││ ● 대화1 (최근)          │  │ │ home-panel — 폭 min(100%,640px)          │         │
││   대화2                │  │ │ 준비단계 CPU런타임›모델›대화 + 제목 +     │         │  584px
││   대화3                │  │ │ 본문/캡션(상태별) + primary 버튼          │  ≤192px │  (본문
││   대화4                │  │ └──────────────────────────────────────────┘         │  높이)
││   대화5                │  │ ↕ 20px
││                        │  │ ● 모델: Qwen2.5-7B-Instruct · CPU · 준비됨   (20px)   │
││                        │  │ ↕ 16px
││                        │  │ 최근 대화                                    (16px)  │
││                        │  │ ↕ 8px
││                        │  │ [세션 아이템 48px]                                    │
││                        │  │ [세션 아이템 48px]  × 최대 5개, 행 간격 4px           │
││                        │  │ [세션 아이템 48px]                                    │
││                        │  │ [세션 아이템 48px]                                    │
││                        │  │ [세션 아이템 48px]                                    │
││                        │  └ 하단 패딩 24px                                        │
│├────────────────────────┴─────────────────────────────────────────────────────────┤
││ ⚡ 진단                  상태바 28px — 전체 폭                                     │   28px
│└────────────────────────────────────────────────────────────────────────────────┘
```

### 4.3 900×580 wireframe (최소 창)

사이드바가 224px로 좁아지고(미디어쿼리 `≤1179px`), 좌우 패딩이 32→20px로 줄며, 최근 대화는
공간 예산에 맞춰 3개까지만 보여준다. 레이아웃 구조 자체는 동일하다.

```
┌──────────────────────────────────────────────────────────────────────┐
│ 사이드바 224px    │ 헤더 48px: 홈            모델 칩(축소) ▾ │ ⌫ │ ⚙  │  580px
│┌───────────────────┼───────────────────────────────────────────────────┤
││[마크+이름]=홈  [+]│ ← 20px →                              ← 20px →   │
││ 🔍 검색            │ ┌ 상단 패딩 24px                                  │
││ ● 대화1            │ │ ┌────────────────────────────────────┐         │
││   대화2            │ │ │ home-panel — 폭 min(100%,640px)     │  ≤192px │  504px
││   대화3            │ │ └────────────────────────────────────┘         │
││                    │ │ ↕20 · ● 모델 요약(20px) · ↕16 · 최근 대화(16px)│
││                    │ │ ↕8 · [세션 아이템 48px] × 3개, 간격 4px         │
││                    │ │ 하단 패딩 24px                                  │
│├───────────────────┴───────────────────────────────────────────────────┤
││ ⚡ 진단              상태바 28px                                        │   28px
│└────────────────────────────────────────────────────────────────────┘
```

### 4.4 세로 예산 수치 증명

중앙 본문(`.conversation`) 높이 = 창 높이 − 상태바 28px − 헤더 48px.

| 창 크기 | 본문 높이 | 상하 패딩 | 순 예산 |
|---|---|---|---|
| 1180×660 | 660 − 28 − 48 = **584px** | 24 + 24 = 48px | 536px |
| 900×580 | 580 − 28 − 48 = **504px** | 24 + 24 = 48px | 456px |

`home-panel` 최댓값(모든 하위 요소가 동시에 존재하는 최악의 경우 — 5.1절 상태 1처럼 준비 단계 표시줄 +
제목 + 2줄 본문 + 캡션 + 버튼 행을 모두 가진 상태)의 높이를 요소별로 더해 증명한다:

| 요소 | 높이 | 비고 |
|---|---|---|
| 패널 상단 패딩 | 16px | |
| 준비 단계 표시줄 | 16px | |
| 간격 | 8px | |
| 제목(16px/600, line-height 1.4) | 24px | 16×1.4=22.4 → 4px 그리드로 24px 확보 |
| 간격 | 4px | |
| 본문(13px/1.55, 최대 2줄) | 40px | 13×1.55≈20.15 → 줄당 20px |
| 간격 | 8px | |
| 캡션/메타(11px, 1줄) | 16px | |
| 간격 | 12px | |
| 액션 행(버튼 32px) | 32px | |
| 패널 하단 패딩 | 16px | |
| **합계** | **192px** | 5절에서 이보다 짧은 상태는 모두 이 값 이하 |

고정 오버헤드(패널 + 모델 요약 줄 + 최근 대화 헤더 + 그 사이 간격, 세션 목록 제외):

```
24(상단패딩) + 192(패널) + 20(간격) + 20(모델 요약 줄) + 16(간격)
  + 16(최근 대화 헤더) + 8(간격) + 24(하단패딩) = 320px
```

세션 아이템은 실측 48px(패딩 `8px 8px 8px 12px` = 상/우/하/좌, 세로 패딩 8+8=16px + 제목 행 16px +
메타 행 min-height 16px), 목록 행 간격 4px. 제목 행 16px가 성립하려면 `.session-title`의 line-height가
16px(또는 그와 동등한 값)이어야 한다 — 목업에서 body 전역 `line-height`를 세션 타이틀이 그대로
상속하면(예: 1.45) 13px 폰트 기준 18.85px로 늘어나 아이템이 48px보다 커지므로, `.session-title`에는
`line-height: 16px`를 명시로 고정한다(목업 CSS에 이미 반영).
n개일 때 목록 높이 = `48n + 4(n−1) = 52n − 4`.

오버헤드 320px에 상하 패딩 48px가 이미 포함되므로, 아래 표는 (패딩을 다시 빼지 않은) **본문 높이**
기준으로 계산한다.

| 창 크기 | 본문 높이 | 오버헤드(패딩 포함) | 목록 가용 공간 | 52n−4 ≤ 가용 공간 | 표시 개수 n | 실제 높이 | 여유 |
|---|---|---|---|---|---|---|---|
| 1180×660 | 584px | 320px | 264px | 52n≤268 → n≤5.15 | **5개** | 256px | 8px |
| 900×580 | 504px | 320px | 184px | 52n≤188 → n≤3.6 | **3개** | 152px | 32px |

두 창 크기 모두 잘림·겹침 없이 성립하며, 결과 개수(5개/3개)는 브리프가 요구한 "최근 대화 3~5개"와
정확히 일치한다. 최근 대화가 3~5개보다 적으면 목록 높이만 줄어들고 아래 여백이 늘어난다(빈 공간을
다른 요소로 채우지 않는다 — 조용한 도구 원칙).

두 기준 크기 사이의 임의 창 높이에는 다음 단일 규칙을 적용한다: **창 높이 ≥ 652px이면 5개, 그 미만이면
3개**. 경계값 652px는 위 산식에서 나온다 — 5개 목록(256px) + 오버헤드(320px) = 본문 높이 576px가
필요하고, 여기에 헤더 48px + 상태바 28px를 더하면 창 높이 652px이다. 651px 이하에서 5개를 그리면
세로 예산을 초과해 스크롤이 생기므로 3개로 줄인다(중간 단계 4개는 두지 않는다 — 규칙 단순화 우선,
3개도 브리프의 "3~5개" 범위 안이다). 창 높이는 CSS `@media (max-height: 651px)`로 판정한다
(목업 동일).

### 4.5 좌우 패딩·칼럼 폭·정렬 규칙 요약

- 홈 콘텐츠 컨테이너: `width: min(100%, 760px); margin: 0 auto;` (메시지 칼럼과 동일 폭).
- 좌우 패딩: 32px(창 폭 ≥1180px) / 20px(창 폭 ≤1179px, 기존 미디어쿼리 임계값 재사용).
- `home-panel`: `width: min(100%, 640px)` — `.error-block`의 `max-width: 640px`와 동일한 상수를 재사용.
- 모델 요약 줄, "최근 대화" 헤더, 세션 아이템은 모두 컨테이너 전체 폭(760px 상한)을 사용한다.
- 모든 텍스트는 좌측 정렬. 버튼 행만 좌측 정렬로 시작(우측 정렬하지 않음 — 모달과 달리 홈은 문서형).
- 모델 요약 줄(`.ready-line`의 텍스트)은 상태바 `.status-model`과 동일하게 **단일 행 말줄임**
  (`overflow:hidden; text-overflow:ellipsis; white-space:nowrap`)을 적용한다 — 4.4절 세로 예산이
  이 줄을 정확히 20px(1줄)로 계산하므로, 매우 긴 GGUF 파일명이 2줄로 늘어나면 예산에 없는 높이가
  추가되어 900×580에서 목록 여유(32px)를 잠식할 수 있다. 줄바꿈을 허용하지 않고 잘라서 1줄을 보장한다.

---

## 5. 상태별 화면 명세

패널 색상은 상태에 따라 자동으로 결정되며 임의로 고르지 않는다: 기본은 뉴트럴(`.home-panel` 또는
`.loading-card`), **재시작 대기만 amber**(`.runtime-restart-notice` 재사용), **실제 실패만 red**
(`.error-block` 재사용). 그 외의 색은 쓰지 않는다 — tokens.css의 "amber=주의·재시작 필요,
red=실제 실패만" 배분 원칙을 그대로 따른 것이다.

모든 상태 공통: 패널 내부 최상단에 준비 단계 표시줄(4.3절 wireframe의 "CPU 런타임 › 모델 › 대화")을
둔다. 각 세그먼트는 `.status-dot`을 재사용한 점 + 12px 텍스트다.

| 세그먼트 상태 | 점 스타일 | 텍스트 색 |
|---|---|---|
| done(완료) | `.status-dot.ready`(칠해진 accent) | `--text-2` |
| current(진행 중) | `.status-dot.ready`(칠해진 accent), 설치/로딩 중이면 `.status-dot.loading`(펄스) | `--text-1`, 600 |
| pending(대기) | `.status-dot`(테두리만, 기본) | `--text-3` |

| 상태 # | CPU 런타임 | 모델 | 대화 |
|---|---|---|---|
| 1~5 (CPU 관련) | current | pending | pending |
| 6, 7 (모델 관련) | done | current | pending |
| 8 (대화 준비됨) | done | done | current |

세그먼트 사이 구분자: `›` (`--text-3`).

### 5.0 상태 판정 우선순위 (공통 전제)

각 상태의 "발동 조건"은 아래 우선순위를 위에서부터 검사해 처음 일치하는 것을 쓴다(10.2절에 의사코드로
재수록). 여러 조건이 동시에 참일 수 있으므로 순서가 중요하다 — 예를 들어 설치가 진행 중이면
"CPU 런타임 없음" 조건도 동시에 참이지만 설치 진행 상태가 우선한다.

```
1. installState.packId === 'cpu' && phase ∈ {downloading, verifying, installing}   → 상태 3 (세부 단계별)
2. (installState.packId === 'cpu' && phase === 'installed' && restartRequired === true) ||
   cpuPack.status === 'replacement-pending'                                        → 상태 4
3. installState.packId === 'cpu' && phase === 'failed'                             → 상태 5 (원인: 설치 실패)
4. cpuPack.status ∈ {'not-installed', 'repair-required'} && packInstaller.loading  → 상태 2
5. cpuPack.status !== 'ready' && packInstaller.error                               → 상태 5 (원인: 오프라인/네트워크)
6. runtime.state.phase === 'error' && isCpuRuntimeRecoveryError(state.error)       → 상태 5 (원인: 실행 중 CPU 런타임 오류)
7. cpuPack.status ∈ {'not-installed', 'repair-required'}                           → 상태 1
8. runtime.state.phase === 'no-model'                                              → 상태 6
9. runtime.state.phase === 'loading'                                               → 상태 7
10. runtime.state.phase ∈ {'ready', 'streaming'}                                   → 상태 8
```

`cpuPack`은 `runtime.runtimePacks.find(p => p.id === 'cpu')`(context 2.3, `RuntimePackStatus`).

**규칙 2에 `replacement-pending`을 포함하는 이유**: `runtimePacks.ts`의 `RuntimePackStatus`에는 설치
완료 후 재시작 전까지 유지되는 `'replacement-pending'` 값이 있다(디스크의 `.transactions/{id}.json`
마커로 판정, `runtime_packs.rs`). 활성 DLL 교체 직후에는 `installState.phase === 'installed'`와
`restartRequired === true`로 상태 4가
켜지지만, 사용자가 "나중에"로 알림만 닫고 홈을 나갔다 다시 들어오면 `installState`는 더 이상 최신이
아닐 수 있어도 `cpuPack.status`는 재시작 전까지 계속 `replacement-pending`이다 — 이 값을 규칙 2에
포함하지 않으면 규칙 7({'not-installed','repair-required'}만 검사)에도 걸리지 않아 판정이 무정의로
빠진다. 5.4절 "나중에" 동작 참조.

**규칙 4가 `repair-required`도 포함하는 이유**: 상태 1의 설치 버튼과 캡션(`{size} · v{version}`)은
카탈로그(`availablePacks`) 데이터가 있어야 성립한다. 복구가 필요한 팩(`repair-required`)도 같은
카탈로그를 통해 재설치하므로, 카탈로그 로딩 중에는 `not-installed`와 동일하게 상태 2(버튼 비활성 +
사유 표시)로 묶는다 — `not-installed`만 검사하면 복구 대상이 카탈로그 없이 상태 1로 빠져 캡션에
표시할 데이터가 없는 상태가 된다.

**`streaming` 위상 처리(규칙 10)**: 실제 `LlmPhase`(`apps/desktop/src/services/nativeRuntime.ts`)는
`'no-model' | 'loading' | 'ready' | 'streaming' | 'error'` 5개 값을 갖는다 — 응답 생성 중에는
`phase`가 `'streaming'`으로 바뀐다(`useNativeRuntime`의 submit-started 처리). 사이드바 홈 버튼은 생성
중에도 항상 눌리므로, 대화가 스트리밍 중인 채로 홈에 들어오는 경로가 실제로 존재한다. 이때 홈은
"대화 준비됨"(상태 8)과 동일하게 표시한다 — 홈은 활성 턴의 진행 상황 자체를
보여주는 화면이 아니라 준비 상태 랜딩이므로, 생성 중이라는 사실은 채팅 화면(상태바·컴포저)에서만
드러내면 충분하다. 모델 요약 줄(5.8절)도 상태 8과 동일하게 "준비됨" 계열 문구를 쓴다(생성 중이라는
이유로 별도 문구를 추가하지 않는다 — 장황한 설명 금지 원칙).

### 5.1 상태 1 — CPU 런타임 없음

- **발동 조건**: 5.0절 규칙 7 — 설치가 진행/완료/실패 이력 없이 `cpuPack.status`가 `not-installed`
  또는 `repair-required`이고, 카탈로그 로딩·오류도 없는 평상시.
- **패널**: `.home-panel`(뉴트럴).
- **제목**: `CPU 런타임 설치가 필요합니다`
- **본문**: `대화를 시작하려면 로컬 CPU 런타임이 필요합니다. 한 번만 설치하면 이후에는 바로 사용할 수 있습니다.`
- **캡션**: `{size} · v{releaseVersion} · 로컬 설치` — 현재 카탈로그 예: `18 MB · v0.1.0 · 로컬 설치`
  (`size`는 `formatBytes(availablePacks.find(p=>p.backend==='cpu').sizeBytes)`, GB는 소수 1자리·MB는 정수).
- **버튼**: primary `CPU 런타임 설치`(`download` 아이콘, 32px) — 클릭 시 기존 설정 패널과 동일한
  `.confirm-dialog.runtime-install-dialog`를 CPU 팩으로 채워 띄운다(9절, 완전 재사용). 확인 시
  `packInstaller.install('cpu')` 호출 → 상태 3으로 전이. 보조 행동 없음(브리프가 요구하지 않음 —
  기존 대화 열람·설정·진단은 항상 켜져 있는 사이드바/헤더로 이미 가능하다).
- **치수**: 패널 폭 `min(100%,640px)`, 패딩 16, 제목 16/600, 본문 13/1.55(max-width 560px),
  캡션 11 `--text-2`, primary 버튼 `.button-primary` 그대로(min-height 32, 패딩 0 12).
- **동기화**: 모델 칩은 "CPU 런타임 설치 후 선택 가능" 상태로 비활성화한다. 상태바 텍스트는
  `CPU 런타임 필요`, 상태점은 뉴트럴이다. 컴포저는 숨김(홈이므로).

### 5.2 상태 2 — CPU 다운로드 정보 확인 중

- **발동 조건**: 5.0절 규칙 4 — `packInstaller.loading === true`(카탈로그 아직 로딩 중).
- **패널**: `.home-panel`(뉴트럴).
- **제목**: `다운로드 정보 확인 중...` (기존 카피 그대로, `.` 3개 포함)
- **본문**: 없음.
- **버튼**: primary `CPU 런타임 설치` **비활성**(`disabled`, `--bg-active`/`--text-3`).
- **비활성 사유**: 버튼 아래 11px `--text-2` 문구 `설치 정보를 불러오는 중에는 설치를 시작할 수 없습니다.`
  (브리프: "설치 버튼은 비활성화하고 이유를 명확히 보여준다"를 만족).
- **치수**: 제목 자리에 `loader-circle`(14px, spin) 아이콘을 제목 앞에 붙인다. 그 외 상태 1과 동일 규격.
- **동기화**: CPU가 아직 준비되지 않았으므로 모델 칩은 비활성화한다. 상태바 텍스트는
  `CPU 런타임 확인 중`, 상태점은 loading이다.

### 5.3 상태 3 — CPU 다운로드 및 설치 중

- **발동 조건**: 5.0절 규칙 1 — `installState.packId === 'cpu'`이고 `phase`가 `downloading` /
  `verifying` / `installing` 중 하나.
- **패널**: `.loading-card` 셸을 그대로 재사용(파일명 자리에 "CPU 런타임" 문구, 그 아래 4px 진행바,
  `.loading-meta` 행). 폭 `min(440px,100%)`(기존 `.loading-card` 규격 그대로), 패딩 20.
- **버튼**: primary 없음(진행 중에는 결정할 것이 없다 — 1절 원칙 2 참조).

3단계는 진행바 채움과 `.loading-meta` 텍스트만 바뀐다:

| 하위 단계 | 제목(강조 텍스트) | `.loading-meta` 좌측 | `.loading-meta` 우측 | 진행바 | 보조 버튼 |
|---|---|---|---|---|---|
| 다운로드 중 | `CPU 런타임을 다운로드하는 중입니다` | `다운로드 중` | mono `{downloaded} / {total} · {percent}%` (현재 카탈로그 예: `6 / 18 MB · 32%` — `formatBytes`는 MB를 정수로 반올림하므로 소수점을 쓰지 않는다) | `downloadedBytes/totalBytes` 비율로 채움 | `취소`(`.button-secondary`) — **다운로드 단계에서만** 노출 |
| 검증 중 | `CPU 런타임을 검증하는 중입니다` | `검증 중` | (없음) | 100% 채움 고정(바이트 신호 없음) | 없음 |
| 설치 중 | `CPU 런타임을 설치하는 중입니다` | `설치 중` | (없음) | 100% 채움 고정 | 없음 |

캡션(모든 하위 단계 공통, `.caption` 재사용): `매니페스트와 파일 체크섬을 확인한 뒤 다음 시작 시 활성화됩니다.`
현재 구현은 고정된 매니페스트 SHA-256과 아카이브 SHA-256을 검증하며 detached signature를 사용하지 않는다.

- **동기화**: 모델 칩은 비활성화한다. 상태바는 하위 단계에 맞춰 `CPU 런타임 다운로드 중` /
  `CPU 런타임 검증 중` / `CPU 런타임 설치 중`으로 표시하고 상태점은 loading이다. 설정 패널을 열면
  (동시에 가능) 동일한 `installState`를 공유하므로 진행률이 정확히 같은 값으로 보인다.

### 5.4 상태 4 — 사용 중인 CPU DLL 교체 완료

- **발동 조건**: 5.0절 규칙 2 — CPU 설치 완료 이벤트의 `restartRequired === true` 또는
  `cpuPack.status === 'replacement-pending'`. 최초 CPU 설치 완료는 worker를 즉시 기동하고 상태 6으로 간다.
- **패널**: `.runtime-restart-notice` 그대로 재사용(amber 좌보더 3px, 배경 `--amber-bg`, 패딩 10).
  준비 단계 표시줄만 블록 최상단에 한 줄 추가한다(다른 재사용 컴포넌트에는 원래 없던 자식 1개).
- **제목(strong)**: `CPU 런타임 교체가 준비되었습니다`
- **본문(p)**: `현재 사용 중인 DLL을 안전하게 교체하려면 앱을 한 번 재시작해야 합니다.`
- **버튼**: primary `지금 재시작`(`restart_runtime_app` 호출) · secondary `나중에`(설치 알림만 dismiss —
  **상태 4를 유지한다**, 상태 1로 되돌리지 않는다). 두 버튼 모두 아이콘 없음 — `.runtime-restart-notice`를
  원본 그대로 재사용하기 때문이다(1절 원칙 4의 예외, 기존 설정 패널의 동일 버튼에도 아이콘이 없다).
  "나중에"가 상태 4를 유지하는 이유: 재시작 전까지는 `cpuPack.status`가 `replacement-pending`
  (재시작 필요, `runtimePacks.ts`)이며 5.0절 규칙 2가 이 상태도 상태 4로 판정하므로, 알림을 닫아도
  다시 홈에 들어오면 동일한 amber 알림이 재노출된다 — "설치가 끝났는데 상태 1(설치 필요)로 되돌아간다"는
  오도를 막기 위해 문구·판정 로직을 함께 정정했다(구버전 초안의 자기모순 수정).
- **치수**: `.panel-actions` 그대로(간격 6px), 버튼 높이 32(primary)/30(secondary).
- **동기화**: 새 런타임은 재시작 전까지 사용할 수 없으므로 모델 칩은 비활성화한다. 상태바 텍스트는
  `재시작 필요`, 상태점은 amber 계열로 표시한다.

### 5.5 상태 5 — CPU 설치 실패 또는 오프라인

- **발동 조건**: 5.0절 규칙 3, 5, 6 중 하나. 원인 분류는 아래 표를 따른다.
- **패널 색상은 발동 규칙에 따라 갈린다** (브리프: "빨간색은 실제 실패에만 사용하고 준비 필요 상태는
  중립색 또는 amber 사용"):
  - **규칙 3(설치를 실제로 시도했다가 실패) · 규칙 6(실행 중 CPU 런타임 오류)** → `.error-block`
    그대로 재사용(좌보더 2px `--red`, 배경 `--red-bg`, radius `0 6px 6px 0`, 패딩 12 16, max-width
    640) — 시도한 작업이 실제로 실패했으므로 red가 맞다. 원인 분류는 검증 실패/디스크 부족/실행 중
    오류/그 외가 여기 해당한다.
  - **규칙 5(설치를 시도하지도 않은 카탈로그 조회 실패 = `distributionError`, 원인: 네트워크/오프라인)**
    → `.home-panel`(뉴트럴) 재사용 — 설치가 실패한 것이 아니라 정보를 못 받아온 것뿐이며, 기존 설정
    패널도 같은 상황을 "이미 설치된 백엔드는 계속 사용할 수 있습니다"라는 중립 문구로 처리한다
    (context 2.3). 제목·본문 카피는 아래 표의 "네트워크/오프라인" 그대로 쓰되, 색만 뉴트럴로 낮춘다.
  준비 단계 표시줄은 두 렌더 모두 블록 최상단에 한 줄 추가한다.
- **버튼**: primary `다시 시도`(`rotate-cw` 아이콘, `.button-primary`) · secondary `진단 보기`
  (`activity` 아이콘, `.button-secondary`) — 색상과 무관하게 두 버튼 모두 동일하게 존재한다. 원인에
  따라 재시도 대상이 다르다: 네트워크/오프라인이면 카탈로그를 다시 불러오고(`packInstaller.refresh()`),
  설치 실패면 `packInstaller.install('cpu')` 재호출, 실행 중 CPU 런타임 오류면 동일하게 CPU 팩
  재설치를 트리거한다(기존 채팅 화면의 대응 문구 "설정에서 검증된 CPU 런타임을 설치한 뒤 앱을
  재시작하세요"와 같은 처방). `진단 보기`는 `diagnosticsOpen = true`로 전환.
- **영어 원문 비노출**: 네이티브 오류 문자열(`installState.error`, `packInstaller.error`,
  `state.error`)은 홈에 그대로 출력하지 않는다. 아래 표의 한국어 문구로만 노출하고, 원문은
  진단 화면에만 표시한다(기존 `NativeDiagnosticsView`가 이미 값 필드를 보여주는 영역을 그대로 쓴다).

**원인 분류 → 문구 매핑** (분류 기준은 오류 문자열의 부분 일치로 판정하는 새 헬퍼
`classifyRuntimeInstallError(message: string)`를 제안한다 — 정확한 하위 문자열은 Rust 쪽 실제
오류 텍스트를 구현 시점에 확인해 조정한다. 아래 substring은 제안값이다):

| 원인 분류 | 감지 기준(제안, 대소문자 무관 부분 일치) | 제목 | 본문 |
|---|---|---|---|
| 네트워크/오프라인 | `network`, `offline`, `timeout`, `dns` | `인터넷 연결을 확인하세요` | `CPU 런타임 정보를 받아오지 못했습니다. 네트워크 연결을 확인한 뒤 다시 시도하세요.` |
| 검증 실패 | `checksum`, `sha-256`, `verify`, `hash` | `다운로드한 파일을 확인하지 못했습니다` | `파일이 손상되었거나 체크섬이 일치하지 않습니다. 다시 시도하면 처음부터 다시 다운로드합니다.` |
| 디스크 공간 부족 | `disk`, `space`, `enospc` | `저장 공간이 부족합니다` | `CPU 런타임을 설치할 공간이 부족합니다. 여유 공간을 확보한 뒤 다시 시도하세요.` |
| 실행 중 CPU 런타임 오류 | `isCpuRuntimeRecoveryError()` = true (규칙 6) | `CPU 런타임을 불러오지 못했습니다` | `검증된 CPU 런타임을 다시 설치하세요.` |
| 그 외/알 수 없음 | 위에 해당하지 않는 모든 경우 | `CPU 런타임을 설치하지 못했습니다` | `알 수 없는 문제로 설치가 중단되었습니다. 다시 시도하거나 진단에서 자세한 내용을 확인하세요.` |

- **치수**: red 렌더는 제목(`error-title`) 13px/600 `--red` + `triangle-alert` 14px, 본문 12px
  `--text-2` line-height 1.6, 액션 간격 8px(`.error-actions`). 뉴트럴 렌더(네트워크/오프라인)는 상태
  1과 동일한 `.home-panel` 치수(제목 16px/600, 본문 13px/1.55, 패딩 16, 액션 간격 8px)를 그대로 쓴다.
- **동기화**: CPU가 준비되지 않은 모든 상태 5에서 모델 칩을 비활성화하고 상태바 텍스트는
  `CPU 런타임 필요`로 표시한다. 상태점 색은 패널과 동일한 기준으로 분기한다 — 규칙 3·6(red 렌더)은
  상태점도 `--red`, 규칙 5·네트워크/오프라인(뉴트럴 렌더)은 상태점도 뉴트럴로 낮춘다.

### 5.6 상태 6 — CPU 준비됨, GGUF 모델 없음

- **발동 조건**: 5.0절 규칙 8 — `runtime.state.phase === 'no-model'`이고 CPU 팩은 `ready`.
- **패널**: `.home-panel`(뉴트럴).
- **제목**: `사용할 GGUF 모델을 선택하세요`
- **본문**: `로컬 GGUF 파일을 선택하면 대화를 시작할 수 있습니다.`
- **캡션**: `지원 형식: .gguf · 모든 추론은 이 PC에서만 실행됩니다.` (기존 채팅 빈 상태 카피 재사용)
- **버튼**: primary `GGUF 모델 선택…`(`folder-open` 아이콘) — `runtime.chooseModel()` 호출(OS 파일
  선택기, `.gguf` 필터). 보조 행동 없음.
- **치수**: 상태 1과 동일 규격.
- **동기화**: 모델 칩 "GGUF 모델 선택…"(미선택 스타일), 상태바 "모델 없음".

### 5.7 상태 7 — 모델 로딩 중

- **발동 조건**: 5.0절 규칙 9 — `runtime.state.phase === 'loading'`.
- **패널**: `.loading-card` 그대로 재사용 (채팅 화면의 모델 로딩 카드와 완전히 동일한 컴포넌트).
- **내용**: 파일명(`.loading-file`, 한 줄 말줄임 처리 — `overflow:hidden; text-overflow:ellipsis;
  white-space:nowrap`, App.css 실측대로 줄바꿈하지 않는다) + `.progress-track`/`.progress-fill`
  (`state.loadingProgress`) + `.loading-meta` 좌측 `모델 로딩 중 · {backend}` 우측 `{percent}%`.
- **버튼**: secondary `로드 취소`만 존재(primary 없음 — 1절 원칙 2), 아이콘 없음(1절 원칙 4의 예외 —
  `.loading-card`를 원본 그대로 재사용). 취소 시 이전 상태로 복귀
  (모델 미선택이었으면 상태 6, 이전 모델이 메모리에 남아 있었으면 상태 8).
- **치수**: 기존 `.loading-card` 규격 그대로(폭 `min(440px,100%)`, 패딩 20).
- **동기화**: 모델 칩도 동시에 스피너+퍼센트 표시(기존 `ChatHeader` 로직 그대로), 상태바 상태점 펄스.

### 5.8 상태 8 — 모델 준비됨 (대화 시작 가능)

- **발동 조건**: 5.0절 규칙 10 — `runtime.state.phase ∈ {'ready', 'streaming'}`(생성 중에도 이 상태로
  표시한다 — 5.0절 "`streaming` 위상 처리" 참조).
- **패널**: `.home-panel`(뉴트럴), 이 상태만 예외적으로 아이콘을 포함한다(`book-open`, 20px, `--accent`)
  — 다른 상태는 본문이 있어 아이콘 없이도 맥락이 충분하지만, 이 상태는 제목+버튼만 있어 시각적
  기준점으로 아이콘 하나를 둔다.
- **제목**: `대화를 시작할 준비가 되었습니다`
- **본문/캡션**: 없음(현재 모델·백엔드는 패널 바로 아래 모델 요약 줄이 이미 보여주므로 중복 표기하지
  않는다 — 장황한 설명 금지 원칙).
- **버튼**: primary `새 대화 시작`(`square-pen` 아이콘) — 3.3절 "생성" 전이를 실행한다.
- **치수**: 패딩 16, 아이콘→제목 간격 8px, 제목→버튼 간격 12px.
- **동기화**: 모델 칩에 모델명 표시, 상태바 "준비됨" + 텔레메트리 값 채워짐.

---

## 6. 전체 한국어 문구 표

| 구역 | 문구 | 비고 |
|---|---|---|
| 준비 단계 표시줄 | `CPU 런타임` / `모델` / `대화` | 세그먼트 라벨, 구분자 `›` |
| 상태 1 제목 | `CPU 런타임 설치가 필요합니다` | |
| 상태 1 본문 | `대화를 시작하려면 로컬 CPU 런타임이 필요합니다. 한 번만 설치하면 이후에는 바로 사용할 수 있습니다.` | |
| 상태 1 캡션 | `{size} · v{version} · 로컬 설치` | 현재 카탈로그 예: `18 MB · v0.1.0 · 로컬 설치`; 값은 하드코딩하지 않는다 |
| 상태 1/6/8 버튼 | `CPU 런타임 설치` / `GGUF 모델 선택…` / `새 대화 시작` | |
| 상태 2 제목 | `다운로드 정보 확인 중...` | 기존 카피 재사용 |
| 상태 2 비활성 사유 | `설치 정보를 불러오는 중에는 설치를 시작할 수 없습니다.` | |
| 상태 3 다운로드 제목 | `CPU 런타임을 다운로드하는 중입니다` | |
| 상태 3 검증 제목 | `CPU 런타임을 검증하는 중입니다` | |
| 상태 3 설치 제목 | `CPU 런타임을 설치하는 중입니다` | |
| 상태 3 단계 라벨 | `다운로드 중` / `검증 중` / `설치 중` | |
| 상태 3 진행값 | `{downloaded} / {total} · {percent}%` | mono, tabular-nums |
| 상태 3 캡션 | `매니페스트와 파일 체크섬을 확인한 뒤 다음 시작 시 활성화됩니다.` | 실제 SHA-256 검증 방식과 일치 |
| 상태 3 취소 버튼 | `취소` | 다운로드 단계에서만 |
| 상태 4 제목 | `CPU 런타임이 준비되었습니다` | |
| 상태 4 본문 | `현재 사용 중인 DLL을 안전하게 교체하려면 앱을 한 번 재시작해야 합니다.` | |
| 상태 4 버튼 | `지금 재시작` / `나중에` | 기존 카피 재사용 |
| 상태 5 제목(네트워크) | `인터넷 연결을 확인하세요` | |
| 상태 5 본문(네트워크) | `CPU 런타임 정보를 받아오지 못했습니다. 네트워크 연결을 확인한 뒤 다시 시도하세요.` | |
| 상태 5 제목(검증 실패) | `다운로드한 파일을 확인하지 못했습니다` | |
| 상태 5 본문(검증 실패) | `파일이 손상되었거나 체크섬이 일치하지 않습니다. 다시 시도하면 처음부터 다시 다운로드합니다.` | |
| 상태 5 제목(디스크) | `저장 공간이 부족합니다` | |
| 상태 5 본문(디스크) | `CPU 런타임을 설치할 공간이 부족합니다. 여유 공간을 확보한 뒤 다시 시도하세요.` | |
| 상태 5 제목(실행 중 오류) | `CPU 런타임을 불러오지 못했습니다` | |
| 상태 5 본문(실행 중 오류) | `검증된 CPU 런타임을 다시 설치하세요.` | |
| 상태 5 제목(기타) | `CPU 런타임을 설치하지 못했습니다` | |
| 상태 5 본문(기타) | `알 수 없는 문제로 설치가 중단되었습니다. 다시 시도하거나 진단에서 자세한 내용을 확인하세요.` | |
| 상태 5 버튼 | `다시 시도` / `진단 보기` | |
| 상태 6 제목 | `사용할 GGUF 모델을 선택하세요` | |
| 상태 6 본문 | `로컬 GGUF 파일을 선택하면 대화를 시작할 수 있습니다.` | |
| 상태 6 캡션 | `지원 형식: .gguf · 모든 추론은 이 PC에서만 실행됩니다.` | |
| 상태 7 진행 라벨 | `모델 로딩 중 · {backend}` | |
| 상태 7 버튼 | `로드 취소` | |
| 상태 8 제목 | `대화를 시작할 준비가 되었습니다` | |
| 모델 요약 줄(모델 있음) | `{modelName} · {backend} · {상태텍스트}` | 상태텍스트는 상태바와 동일 어휘(준비됨/모델 로딩 중/모델 없음 등) |
| 모델 요약 줄(모델 없음) | `모델 없음` | |
| 최근 대화 헤더 | `최근 대화` | |
| 최근 대화 빈 상태 | `아직 대화가 없습니다.` | 절제된 한 줄, 아이콘 없음 |
| 홈 진입 버튼 aria-label | `홈으로 이동` | 사이드바 앱 이름 버튼 |
| 헤더 제목(홈 표시 중) | `홈` | |

---

## 7. 버튼 동작·포커스 표

| 버튼/컨트롤 | 클릭 결과 | 키보드 | 진입 시 포커스 | 전환 후 포커스 | aria |
|---|---|---|---|---|---|
| 사이드바 홈 버튼(앱 마크+이름) | `homeOpen=true`, `diagnosticsOpen=false`, 대기 드래프트 폐기 | 포커스 후 Enter/Space | — | 홈의 primary 버튼(있으면) 또는 패널 제목(`tabindex=-1`, 없으면) | `aria-label="홈으로 이동"`, `homeOpen`일 때 `aria-current="page"` 추가(현재 뷰임을 SR에 알림 — 헤더 설정 아이콘의 `aria-pressed` 패턴과 동등한 목적, 9.3절) |
| 사이드바 새 대화 아이콘 / `Ctrl+N` | 준비 완료면 드래프트를 즉시 열고, 미완료면 홈으로 이동(3.4절 안 B) | `Ctrl+N` 전역 | — | 드래프트의 컴포저 또는 홈의 primary 버튼 | 아이콘 버튼 `aria-label="새 대화"` |
| 홈 primary — `CPU 런타임 설치`(상태 1) | 설치 확인 다이얼로그(`.runtime-install-dialog`) 오픈 | Enter/Space | 홈 진입 시 자동 포커스 대상 | 다이얼로그의 첫 포커스 가능 요소(`다운로드 및 설치` 버튼) | `aria-label="CPU 런타임 설치"` |
| 설치 확인 다이얼로그 `다운로드 및 설치` | `packInstaller.install('cpu')` 호출, 상태 3으로 전이 | Enter | 다이얼로그 오픈 시 | 다이얼로그 닫힘 → 상태 3 패널 영역(포커스는 이동시키지 않고 전용 live status로만 알림 — 8절) | — |
| 설치 확인 다이얼로그 `취소` | 다이얼로그 닫기, 설치 시작 안 함 | Enter, Esc | — | 상태 1 primary 버튼으로 복귀 | — |
| 홈 비활성 primary(상태 2) | 없음(클릭 무시) | native `disabled`로 렌더 — **포커스도 받지 않는다**(탭 순서에서 제외, 목업과 동일) | — | — | `aria-disabled="true"`, `disabled` |
| 상태 3 `취소`(다운로드 중) | `cancel_runtime_pack_install` 호출 | Enter/Space | — | 상태 1 primary 버튼으로 포커스 이동(취소 후 자연스러운 재시도 지점) | `aria-label="다운로드 취소"` |
| 상태 4 `지금 재시작` | `restart_runtime_app` 호출(앱 재기동) | Enter | 상태 4 진입 시 자동 포커스 | — (앱 재시작) | `aria-label="지금 재시작"` |
| 상태 4 `나중에` | 설치 완료 알림 dismiss(상태 4를 유지 — 5.4절, 5.0절 규칙 2) | Enter, Esc | — | 상태 4 primary 버튼(`지금 재시작`)으로 포커스 유지 | `aria-label="나중에"` |
| 상태 5 `다시 시도` | 원인별 재시도(5.5절) | Enter | 상태 5 진입 시 자동 포커스 | 재시도 시작 시 포커스 유지, 실패 문구가 갱신되면 전용 live status로만 알림 | `aria-label="다시 시도"` |
| 상태 5 `진단 보기` | `diagnosticsOpen=true` | Enter | — | 진단 화면 제목(`h1`, `tabindex=-1`) | `aria-label="진단 보기"` |
| 상태 6 `GGUF 모델 선택…` | `runtime.chooseModel()`(OS 파일 선택기) | Enter | 상태 6 진입 시 자동 포커스 | 파일 선택기 닫힘 → 선택 시 상태 7로 전이, 취소 시 포커스 그대로 | `aria-label="GGUF 모델 선택"` |
| 상태 7 `로드 취소` | 모델 로딩 취소 | Enter | — (primary가 없는 진행 상태 — 8.2절 (A). 패널 컨테이너/`.loading-file`로 포커스) | 취소 후 이전 상태(6 또는 8) primary 버튼 | `aria-label="로드 취소"` |
| 상태 8 `새 대화 시작` | 드래프트 생성(3.3절), `homeOpen=false` | Enter | 상태 8 진입 시 자동 포커스 | 채팅 화면 컴포저 입력창 | `aria-label="새 대화 시작"` |
| 최근 대화 항목 | `workspace.select(id)`, `homeOpen=false`, 대기 드래프트 폐기 | Tab 이동 후 Enter/Space | — | 채팅 화면 컴포저 입력창 | 세션 항목과 동일(`.session-item`, 기존 규칙 그대로) |
| 헤더 모델 칩(홈에서도 노출) | CPU 준비 완료 상태 6~8에서만 `runtime.chooseModel()` | 기존과 동일 | — | 기존과 동일 | 상태 1~5에서는 `disabled`, `aria-label="CPU 런타임 설치 후 모델 선택 가능"`; 상태 6~8에서만 기존 동작 사용 |
| 헤더 초기화(⌫) 아이콘 | 홈/진단 표시 중에는 **비활성** | — | — | — | `disabled` — 10.3절 "기존 동작 변경점" |
| 헤더 설정(⚙) 아이콘 | 기존과 동일(설정 패널 토글) | `Ctrl+,` | — | 기존과 동일 | 기존과 동일 |

---

## 8. 접근성

### 8.1 aria-live 정책

- 홈 패널 전체는 live region으로 만들지 않는다. 진행률과 바이트처럼 자주 바뀌는 DOM을 감싸면
  스크린 리더가 모든 갱신을 반복해서 읽을 수 있다. 홈 뷰 안에 시각적으로 숨긴 전용 노드
  `<p className="sr-only" aria-live="polite" aria-atomic="true">`를 하나 두고, 상태 전이 때 간결한
  문장 하나만 갱신한다(예: `CPU 런타임 다운로드를 시작했습니다`).
- **진행률은 매 이벤트마다 알리지 않는다.** 시각적으로는 `runtime-pack-install-progress` 이벤트마다
  진행바·퍼센트 숫자를 즉시 갱신하되, 스크린 리더 알림은 다음 두 경우로만 제한한다: (a) 단계가 바뀔 때
  (다운로드→검증→설치), (b) 다운로드 퍼센트가 10%p 단위로 넘어갈 때(`0, 10, 20 … 100`). 초당 여러 번
  들어오는 바이트 이벤트를 그대로 읽게 하지 않는다. 이 임계값을 넘을 때만 전용 live status의 문자열을
  `CPU 런타임 다운로드 30%`처럼 교체한다.
- 진행바에는 `role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="{percent}"
  aria-label="{단계 라벨}"`를 둔다(검증/설치 단계처럼 세부 퍼센트가 없을 때는 `aria-valuenow="100"`,
  `aria-label`만 단계명으로 갱신).

### 8.2 초기 포커스·포커스 이동

포커스 규칙은 두 경로를 구분해서 정의한다 — (A) 홈 바깥에서 홈으로 들어오는 **최초 진입**, (B) 홈이
열린 채로 판정값(`HomeReadiness`)이 바뀌는 **내부 자동 전이**. 둘 다 최종적으로 "그 상태에 맞는 다음
행동 지점으로 포커스를 보낸다"는 결과는 같지만, (B)는 배경 이벤트(다운로드 완료, 모델 로드 완료 등)로
사용자가 아무것도 누르지 않은 채 화면이 바뀔 수 있다는 점이 다르다.

- **(A) 최초 진입** (`homeOpen`이 `false→true`로 바뀌는 경로 — 콜드 스타트, 사이드바 홈 버튼,
  준비 미완료 상태의 `Ctrl+N`/새 대화 버튼): **그 순간 렌더링되는 상태의 primary 버튼**으로 포커스를 이동한다. primary가
  없거나 비활성인 상태(2, 3, 7 — 8.2절 표의 "진입 시 포커스"가 `—`인 행)에서는 패널 컨테이너 자체
  (`tabindex="-1"`)로 포커스를 이동해 스크린 리더가 패널 텍스트부터 읽게 한다. 상태 7이 여기 포함되는
  이유: `로드 취소`는 secondary이지 primary가 아니므로(1절 원칙 2 — 진행 중에는 primary 버튼이 없다),
  다른 진행 중 상태(2, 3)와 동일하게 취급한다(7절 표의 상태 7 행도 이에 맞춰 `—`로 정정).
- **(B) 내부 자동 전이**: 기본값은 포커스를 강제로 옮기지 않는 것이다 — 사용자가 마지막으로 조작한
  요소 근처에 자연스럽게 남긴다(예: 다이얼로그의 `다운로드 및 설치` 클릭 → 상태 3 진입, 포커스는
  다이얼로그가 닫힌 자리 근처에 남고 `aria-live`로만 알림). 다음 두 경우에만 예외로 새 primary
  버튼으로 포커스를 이동한다 — **새로운 primary가 나타나고, 그 직전에 사용자가 누르고 있던 컨트롤이
  이미 화면에서 사라진 배경 전이**이기 때문이다:
  - **3(설치 중) → 4(설치 완료)**: 설치 진행 중에는 primary 버튼 자체가 없었으므로(1절 원칙 2), 완료
    시 새로 나타나는 `지금 재시작`으로 포커스를 이동해야 재시작 흐름이 끊기지 않는다.
  - **7(모델 로딩 중) → 8(모델 준비됨)**: 로딩 중에는 secondary `로드 취소`만 있었고 이미 사라졌으므로,
    새로 나타나는 `새 대화 시작`으로 포커스를 이동한다.
  그 외 전이(1→3 설치 시작, 3의 하위 단계 전환, 2→1/5/6, 8의 스트리밍 갱신 등)는 포커스를 유지한다.
  8.5절은 포커스 중이던 컨트롤 자체가 DOM에서 사라지는 경우의 별도 폴백을 정의한다.
- 홈을 벗어나 채팅으로 이동할 때(최근 대화 클릭, 상태 8 "새 대화 시작")는 항상 메시지 입력창으로
  포커스를 이동한다(기존 컴포저 포커스 관례와 동일).

### 8.3 스크린 리더 낭독 문구

- 홈 진입 시 `<main>` 랜드마크 `aria-label`을 `"홈"`으로 바꾼다(현재 진단 화면 진입 시에도 동일하게
  `"진단"`으로 바뀌어야 하나 실제로는 `"대화"`로 고정돼 있다 — 홈 도입을 계기로 진단 화면에도 함께
  바로잡을 것을 권장한다. 10.3절).
- 준비 단계 표시줄은 스크린 리더에 `"준비 단계: CPU 런타임 완료, 모델 진행 중, 대화 대기"`처럼
  하나의 문장으로 합쳐 읽히도록 `aria-label`을 표시줄 컨테이너에 별도로 둔다(점 3개를 개별 읽지 않게
  각 세그먼트의 개별 `aria-hidden="true"` 처리 + 컨테이너 레벨 요약 라벨).
- 캡션·본문 텍스트는 시각 텍스트와 스크린 리더 텍스트를 동일하게 유지한다(별도 sr-only 텍스트를
  추가하지 않는다 — 이미 문구 자체가 원인과 다음 행동을 설명하도록 6절에서 작성했다).

### 8.4 포커스 링

전역 규칙을 그대로 따른다 — 새 컴포넌트를 포함해 별도 규칙을 만들지 않는다.

```css
:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
```

`prefers-reduced-motion: reduce`에서는 준비 단계 표시줄의 `current` 세그먼트 펄스(`.status-dot.loading`
재사용)도 기존 전역 규칙에 의해 자동으로 꺼진다(추가 규칙 불필요).

### 8.5 포커스 대상이 사라지는 경우의 폴백

8.2절의 "내부 자동 전이 시 포커스를 유지한다" 원칙에는 포커스 중이던 컨트롤 자체가 전이로 인해
DOM에서 제거되는 경우의 예외가 하나 있다. 대표 사례: 상태 3 다운로드 단계에서 사용자가 `취소` 버튼에
포커스를 두고 있다가, 다운로드가 자연스럽게 100%에 도달해 **검증 중 단계로 자동 전이**되면 `취소`
버튼은 렌더링에서 사라진다(5.3절 표 — 취소는 다운로드 단계에서만 존재). 이때 포커스 대상이 사라져
브라우저 포커스가 `<body>`로 유실되면 스크린 리더 사용자는 다음 초점을 완전히 잃는다.

**규칙**: 포커스 중이던 요소가 전이로 제거될 때는 8.2절의 "유지" 기본값 대신, 그 상태의 패널
컨테이너/제목(`tabindex="-1"`)으로 포커스를 이동한다 — 검증 중 단계 진입 시 `.loading-file`
(`tabindex="-1"`)로 이동하는 것이 그 예다. 새 primary가 없는 전이이므로 8.2절의 두 예외
(3→4, 7→8)와는 별개의 규칙이다.

---

## 9. 컴포넌트 목록

### 9.1 재사용 (기존 클래스·컴포넌트 이름 그대로)

| 이름 | 재사용 방식 |
|---|---|
| `Sidebar`, `ChatHeader`, `StatusBar`, `NativeSettingsPanel`, `ConfirmDialog` | 변경 없이 그대로 렌더링. `Sidebar`만 `onHome` prop이 추가된다(10.3절). |
| `.button-primary` / `.button-secondary` / `.icon-button` | 홈의 모든 버튼이 그대로 사용 |
| `.status-dot`(+ `ready`/`loading` 변형) | 준비 단계 표시줄, 모델 요약 줄 |
| `.loading-card` / `.loading-file` / `.progress-track` / `.progress-fill` / `.loading-meta` | 상태 3(다운로드/검증/설치), 상태 7(모델 로딩) |
| `.error-block` / `.error-title` / `.error-actions` | 상태 5(규칙 3·6 — 실제 설치 실패/실행 중 오류만; 규칙 5의 네트워크/오프라인 원인은 `.home-panel`을 재사용한다, 5.5절) |
| `.runtime-restart-notice` / `.panel-actions` | 상태 4 |
| `.confirm-dialog.runtime-install-dialog` / `.dialog-scrim` | 상태 1의 설치 확인 다이얼로그 |
| `.session-item`(및 하위 `.session-title`/`.session-meta`/`.generating`/`.queued`) | 최근 대화 목록 각 행 |
| `.caption` | 상태 6 캡션 등 |
| `formatBytes(value)` (기존 두 곳의 중복 함수와 동일 로직 — GB는 소수 1자리, **MB는 정수로 반올림**) | 상태 1/3 바이트 표기(소수점 없는 MB 표기가 정상, 5.3절 예시 참조) |
| Lucide 아이콘 `download`, `folder-open`, `square-pen`, `loader-circle`, `triangle-alert`,
  `activity`, `box`, `book-open`, `check` | 기존 세트 그대로 |

### 9.2 신규 컴포넌트

| 이름 | 위치(제안) | props | 구조 |
|---|---|---|---|
| `NativeHomeView` | `apps/desktop/src/components/NativeHomeView.tsx` | `{ readiness: HomeReadiness (5.0절 판정 결과), modelName, backend, statusText, sessions: Session[], installState, availablePacks, distributionError, distributionLoading, onInstallCpu(), onRetryCpuInstall(), onCancelInstall(), onRestart(), onDismissInstall(), onChooseModel(), onCancelModelLoad(), onStartConversation(), onOpenDiagnostics(), onSelectSession(id) }` | `<div className="home-view"><HomeStatusPanel/><HomeModelLine/><HomeRecentSection/><HomeLiveStatus/></div>` — 전체 뷰가 아닌 `HomeLiveStatus`만 `aria-live`를 가지며 `NativeDiagnosticsView`와 형제 컴포넌트 |
| `HomeStatusPanel` | 같은 파일 또는 `HomeStatusPanel.tsx` | `{ readiness: HomeReadiness, ...상태별 데이터 }` | 내부에서 `readiness`값에 따라 5.1~5.8절 중 하나의 마크업을 렌더. 준비 단계 표시줄(`HomeProgressTrail`)을 최상단에 항상 포함 |
| `HomeProgressTrail` | 같은 파일 | `{ current: 'runtime' \| 'model' \| 'chat' }` | 3세그먼트 인라인 표시줄, `.status-dot` 재사용 |
| `HomeModelLine` | 같은 파일 | `{ modelName?: string, backend?: string, statusText: string }` | `.ready-line`과 동일한 마크업(상태점+텍스트) 재사용, 홈 전용 래퍼만 신규 |
| `HomeRecentSection` | 같은 파일 | `{ sessions: Session[], onSelect(id) }` | 헤더 `<h2>최근 대화</h2>` + `.home-recent-list`(신규 wrapper, `.session-item` 재사용) + 빈 상태 `<p className="home-recent-empty">` |
| `HomeLiveStatus` | 같은 파일 | `{ announcement: string }` | `<p className="sr-only" aria-live="polite" aria-atomic="true">`; 상태 전이와 10%p 진행 임계값에서만 문자열 갱신 |
| `classifyRuntimeInstallError(message: string)` | `apps/desktop/src/services/runtimePacks.ts`(확장) 또는 `services/installErrors.ts`(신규) | `(message: string) => 'network' \| 'verification' \| 'disk' \| 'unknown'` | 5.5절 표의 부분 문자열 매칭. 순수 함수, 컴포넌트 아님 |

신규 CSS 클래스(구현 시 `App.css`에 추가할 이름만 제안, 실제 규칙 값은 4~5절 치수를 그대로 옮긴다):
`.home-view`, `.home-panel`, `.home-panel-title`, `.home-panel-body`, `.home-panel-meta`,
`.home-panel-actions`, `.home-progress`, `.home-progress-step`, `.home-progress-sep`,
`.home-recent`, `.home-recent-header`, `.home-recent-list`, `.home-recent-empty`,
`.app-name-button`(9.3절 참조).

### 9.3 `Sidebar`의 필요한 변경

- `SidebarProps`에 `onHome(): void` 추가.
- `<span className="app-name">돌쇠</span>` → `<button type="button" className="app-name-button" onClick={onHome} aria-label="홈으로 이동">`(내부에 기존 `app-mark` SVG + 텍스트를 그대로 포함).
- 신규 CSS(치수 확정, 그대로 구현):
  ```css
  .app-name-button { display: flex; min-width: 0; flex: 1; align-items: center; gap: 8px;
    height: 32px; padding: 0 4px; margin-left: -4px; border-radius: 4px;
    background: transparent; text-align: left; cursor: pointer; }
  .app-name-button:hover { background: var(--bg-hover); }
  .app-name-button.active { background: var(--bg-active); color: var(--accent); }
  ```
  `homeOpen`이 true일 때 `active` 클래스를 붙인다(기존 `.icon-button.active`와 동일한 시각 원리).
- 시각 스타일(`active` 클래스)만으로는 스크린 리더가 "지금 홈을 보고 있다"는 상태를 알 수 없다 —
  헤더 설정 아이콘이 이미 `aria-pressed`로 토글 상태를 노출하는 것과 같은 이유로, `homeOpen`일 때
  `app-name-button`에 `aria-current="page"`를 추가한다. 사이드바 푸터의 "진단" 버튼에도 동일 패턴을
  적용한다(`diagnosticsOpen`일 때 `aria-current="page"`) — 이번 홈 도입을 계기로 두 진입점 모두
  현재 뷰 상태를 노출하도록 맞춘다(7절 표).

---

## 10. 구현 노트

### 10.1 뷰 라우팅 방식

`NativeApp.tsx`의 `NativeWorkspace`에 `diagnosticsOpen`과 대칭인 `homeOpen` state를 추가한다.
`useState<boolean>(true)`로 **초기값을 `true`**로 둔다 — 이것이 3.1절 "콜드 스타트는 항상 홈"을
구현하는 지점이다. 두 플래그는 상호 배타적으로 다룬다(한쪽을 열 때 다른 쪽을 닫는다).

```tsx
const [homeOpen, setHomeOpen] = useState(true);
const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);

function goHome() { setHomeOpen(true); setDiagnosticsOpen(false); discardDraftIfAny(); }
function openDraft() { setHomeOpen(false); setDiagnosticsOpen(false); workspace.openDraft(); }
function startConversationFlow() {
  if (readiness.kind === "ready") openDraft();
  else goHome();
}
function openDiagnostics() { setDiagnosticsOpen(true); setHomeOpen(false); }
function selectConversation(id: string) {
  void workspace.select(id);
  setHomeOpen(false); setDiagnosticsOpen(false); discardDraftIfAny();
}
```

중앙 영역:

```tsx
<main className="conversation" aria-label={homeOpen ? "홈" : diagnosticsOpen ? "진단" : "대화"}>
  {homeOpen
    ? <NativeHomeView ... />
    : diagnosticsOpen
      ? <NativeDiagnosticsView state={state} runtimePack={selectedRuntimePack} />
      : <MessageList ... />}
</main>
{!diagnosticsOpen && !homeOpen && <Composer ... />}
```

헤더 제목/이름 변경/초기화 가드:

```tsx
const title = homeOpen ? "홈" : diagnosticsOpen ? "진단" : current?.title ?? "새 대화";
const canEditTitle = !homeOpen && !diagnosticsOpen && Boolean(current);
// ChatHeader에 onRename={canEditTitle ? (...) => ... : undefined}
// ChatHeader에 onReset={canEditTitle ? () => openConversationDialog("reset", current!.id) : undefined}
```

`ChatHeader`는 `onReset`이 없을 때 초기화 아이콘을 비활성화하도록(또는 `disabled` prop을 받도록)
작은 변경이 필요하다 — 현재는 항상 클릭 가능한 아이콘 버튼이므로, `onReset`이 `undefined`일 때
`disabled`를 전달하는 한 줄을 추가한다. 이는 홈뿐 아니라 기존 진단 화면의 잠재적 오조작도 함께
바로잡는다(7절 표, 8.3절에서 이미 언급).

이 방식은 홈을 진단과 완전히 대칭인 "중앙 영역 교체 뷰"로 다루며, 사이드바·상태바·설정 패널은
그대로 유지된다 — 컨텍스트 문서 2.2절이 권장한 패턴을 그대로 따른 것이다.

### 10.2 상태 판정 우선순위(재수록)

5.0절의 우선순위를 그대로 코드화한다. 이 로직은 `NativeHomeView`가 아니라 `NativeApp.tsx`(또는
전용 훅 `useHomeReadiness()`)에서 계산해 하나의 판정값(`HomeReadiness` — 상태 1~8 중 하나 + 원인
분류 등 부가 데이터)으로 내려준다. `NativeHomeView`는 이 판정값을 스위치해 렌더링만 담당한다.
규칙 10 구현 시 `phase === 'ready'`만 검사하지 않도록 주의한다 — `LlmPhase`에는 `'streaming'`도
존재하므로(5.0절 "`streaming` 위상 처리" 참조) `phase === 'ready' || phase === 'streaming'`로 검사해야
생성 중 홈 진입이 무정의 상태로 빠지지 않는다.

### 10.3 기존 동작 변경점 목록

1. **`Ctrl+N`과 사이드바 새 대화 버튼이 더 이상 `workspace.create()`를 즉시 호출하지 않는다.**
   둘 다 `startConversationFlow()`를 호출해 준비 완료면 드래프트를 열고, 미완료면 홈으로 이동한다
   (3.4절 안 B).
2. **기존 `workspace.create()` → `workspace.submit()` 연쇄 호출은 사용하지 않는다.** 드래프트의 첫
   메시지는 신규 단일 명령 `workspace.startDraft(prompt)`가 대화와 첫 턴을 하나의 트랜잭션으로 저장한다.
   부트스트랩과 마지막 대화 삭제도 빈 대화를 자동 생성하지 않도록 백엔드 계약을 함께 바꾼다(3.3절).
3. **사이드바 앱 이름이 정적 텍스트에서 버튼으로 바뀐다** (`onHome` prop 추가, 9.3절).
4. **콜드 스타트 진입점이 바뀐다**: 기존에는 즉시 채팅 화면(빈 대화 또는 마지막 대화)이었다면,
   이제 항상 홈이다.
5. **헤더 초기화 아이콘이 홈/진단 표시 중에는 비활성화된다** (10.1절 — 기존 진단 화면에도 함께
   적용 권장, 회귀가 아니라 잠재 결함 수정).
6. **홈 진입 시 런타임 팩 카탈로그를 refresh해야 한다.** 현재 `packInstaller.refresh()`는 설정
   패널이 열릴 때만 호출된다(`NativeApp.tsx:80-82`, `useEffect(() => { if (settingsOpen) void
   packInstaller.refresh(); }, [packInstaller.refresh, settingsOpen])`). 홈이 상태 1/2/5의 판정에
   같은 카탈로그 데이터를 쓰므로, 다음과 같이 `homeOpen`도 트리거 조건에 추가한다:
   ```tsx
   useEffect(() => {
     if (settingsOpen || homeOpen) void packInstaller.refresh();
   }, [packInstaller.refresh, settingsOpen, homeOpen]);
   ```
   이렇게 하지 않으면 사용자가 오프라인 상태에서 앱을 켰다가 온라인으로 전환한 뒤 홈으로 돌아와도
   상태 5(실패/오프라인)에 머무는 낡은 화면이 보일 수 있다.
7. **최근 대화 정렬 가정**: 홈의 최근 대화 목록은 사이드바와 동일하게 `updatedAt` 내림차순으로
   이미 정렬되어 있다고 가정한다(사이드바 세션 목록의 실제 정렬 로직은 이번 조사 범위 밖이므로,
   구현 시 정렬 기준이 다르면 홈에서 별도로 정렬해야 한다).
8. **상태바 상태 텍스트·상태점 판정이 `HomeReadiness`와 동기화된다.** 상태 1은 `CPU 런타임 필요`,
   상태 2는 `CPU 런타임 확인 중`, 상태 3은 현재 설치 하위 단계, 상태 4는 `재시작 필요`, 상태 5는
   `CPU 런타임 필요`를 표시한다. 규칙 3·6의 실제 실패만 red, 확인·진행은 loading, 재시작은 amber,
   미설치·오프라인은 뉴트럴 상태점을 사용한다. 홈 패널과 상태바가 별도 조건문으로 갈라지지 않도록
   같은 `HomeReadiness`에서 표시 모델을 계산한다.
9. **헤더 모델 칩은 CPU 준비 여부를 함께 검사한다.** `modelSelectDisabled`는 상태 1~5에서 true이고
   상태 6~8에서만 false다. 파일 선택은 즉시 `llm_load_model`을 호출하므로 런타임 준비 전에는 모델을
   미리 선택하게 하지 않는다.

---

## 11. 목업 매핑 표

`docs/design/mockups/home-mockup.html?state=<값>&theme=<light|dark>`.

| `state=` 값 | 대응하는 스펙 상태 | 비고 |
|---|---|---|
| `runtime-missing` | 5.1 상태 1 | 기본값(쿼리 없을 때) |
| `runtime-checking` | 5.2 상태 2 | primary 비활성 |
| `runtime-downloading` | 5.3 상태 3(다운로드 중) | 취소 버튼 노출 |
| `runtime-verifying` | 5.3 상태 3(검증 중) | |
| `runtime-installing` | 5.3 상태 3(설치 중) | |
| `runtime-installed` | 5.4 상태 4 | amber |
| `runtime-failed-network` | 5.5 상태 5 · 네트워크 원인(규칙 5 = 설치 미시도) | **뉴트럴**(`.home-panel`) — 유일하게 red가 아닌 상태 5 변형 |
| `runtime-failed-verify` | 5.5 상태 5 · 검증 실패 원인 | red |
| `runtime-failed-disk` | 5.5 상태 5 · 디스크 원인 | red |
| `runtime-failed-recovery` | 5.5 상태 5 · 실행 중 CPU 런타임 오류 | red |
| `runtime-failed-unknown` | 5.5 상태 5 · 그 외 | red |
| `model-missing` | 5.6 상태 6 | |
| `model-loading` | 5.7 상태 7 | |
| `ready` | 5.8 상태 8 | 최근 대화 5개(기본 창 기준) |
| `ready-empty-recent` | 5.8 상태 8 + "최근 대화 없음" | 절제된 빈 상태 확인용 |

모든 `state=` 값은 `&theme=light|dark`를 함께 받는다. 창 크기 재현은 브라우저 창 크기 조정으로
확인한다(참고 스크린샷 명령: `msedge --headless=new --window-size=1180,660 ...` /
`--window-size=900,580 ...`, 기존 `mockup.html` 관례와 동일).
