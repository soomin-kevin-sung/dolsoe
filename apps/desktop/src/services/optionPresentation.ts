interface OptionPresentation {
  description: string;
  effect(value: number): string;
}

const integer = new Intl.NumberFormat("ko-KR", { maximumFractionDigits: 0 });
const decimal = new Intl.NumberFormat("ko-KR", { maximumFractionDigits: 2 });

const presentations: Record<string, OptionPresentation> = {
  "--n-predict": {
    description: "한 응답에서 생성할 수 있는 최대 길이를 정합니다.",
    effect: (value) => `최대 ${integer.format(value)} 토큰 생성`,
  },
  "--temp": {
    description: "답변의 무작위성과 다양성을 조절합니다.",
    effect: (value) => {
      if (value === 0) return "가장 일관된 출력";
      if (value <= 0.4) return "낮은 다양성";
      if (value <= 0.9) return "균형 잡힌 다양성";
      if (value <= 1.3) return "높은 다양성";
      return "매우 높은 다양성";
    },
  },
  "--top-p": {
    description: "다음 토큰 후보의 누적 확률 범위를 제한합니다.",
    effect: (value) => value >= 1 ? "후보 범위 제한 안 함" : `누적 확률 ${decimal.format(value * 100)}% 사용`,
  },
  "--seed": {
    description: "생성 결과의 난수 기준을 정합니다.",
    effect: (value) => value === -1 ? "응답마다 무작위" : `시드 ${integer.format(value)} 사용`,
  },
  "--top-k": {
    description: "확률이 높은 토큰 후보의 개수를 제한합니다.",
    effect: (value) => value === 0 ? "후보 개수 제한 안 함" : `상위 ${integer.format(value)}개 후보 사용`,
  },
  "--min-p": {
    description: "확률이 지나치게 낮은 토큰 후보를 제외합니다.",
    effect: (value) => value === 0 ? "최소 확률 필터 사용 안 함" : `최고 확률 대비 ${decimal.format(value * 100)}% 미만 제외`,
  },
  "--repeat-last-n": {
    description: "반복 여부를 검사할 이전 토큰 범위를 정합니다.",
    effect: (value) => value === -1 ? "전체 컨텍스트 검사" : value === 0 ? "반복 검사 사용 안 함" : `최근 ${integer.format(value)}개 토큰 검사`,
  },
  "--repeat-penalty": {
    description: "같은 표현이 반복되는 정도를 조절합니다.",
    effect: (value) => value === 1 ? "반복 페널티 사용 안 함" : value > 1 ? `반복 억제 ${decimal.format(value)}배` : "반복을 더 허용",
  },
  "--frequency-penalty": {
    description: "자주 등장한 토큰의 재사용을 줄입니다.",
    effect: (value) => value === 0 ? "빈도 페널티 사용 안 함" : value > 0 ? `빈도 기반 반복 억제 ${decimal.format(value)}` : `빈도 기반 반복 허용 ${decimal.format(Math.abs(value))}`,
  },
  "--presence-penalty": {
    description: "이미 등장한 토큰이 다시 나오는 것을 줄입니다.",
    effect: (value) => value === 0 ? "존재 페널티 사용 안 함" : value > 0 ? `등장한 토큰 억제 ${decimal.format(value)}` : `등장한 토큰 재사용 허용 ${decimal.format(Math.abs(value))}`,
  },
  "--ctx-size": {
    description: "모델이 한 번에 기억할 대화 문맥의 길이입니다.",
    effect: (value) => `${integer.format(value)} 토큰 문맥 사용`,
  },
  "--batch-size": {
    description: "프롬프트를 한 번에 처리할 논리 배치 크기입니다.",
    effect: (value) => `최대 ${integer.format(value)} 토큰씩 처리`,
  },
  "--ubatch-size": {
    description: "실제 계산에 나누어 넣는 물리 배치 크기입니다.",
    effect: (value) => `${integer.format(value)} 토큰 단위로 계산`,
  },
  "--threads": {
    description: "추론 계산에 사용할 CPU 스레드 수입니다.",
    effect: (value) => `CPU 스레드 ${integer.format(value)}개 사용`,
  },
};

export function describeOption(flag: string, value: number) {
  const presentation = presentations[flag];
  return presentation ? {
    description: presentation.description,
    effect: presentation.effect(value),
  } : {
    description: "llama.cpp 실행 옵션입니다.",
    effect: `현재 값 ${decimal.format(value)}`,
  };
}
