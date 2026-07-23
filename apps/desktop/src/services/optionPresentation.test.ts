import { describe, expect, it } from "vitest";

import { describeOption } from "./optionPresentation";

describe("describeOption", () => {
  it("explains special values instead of only echoing them", () => {
    expect(describeOption("--seed", -1).effect).toBe("응답마다 무작위");
    expect(describeOption("--top-k", 0).effect).toBe("후보 개수 제한 안 함");
    expect(describeOption("--top-p", 1).effect).toBe("후보 범위 제한 안 함");
    expect(describeOption("--repeat-last-n", -1).effect).toBe("전체 컨텍스트 검사");
    expect(describeOption("--repeat-penalty", 1).effect).toBe("반복 페널티 사용 안 함");
  });

  it("describes ordinary values with their operational meaning", () => {
    expect(describeOption("--seed", 42).effect).toBe("시드 42 사용");
    expect(describeOption("--ctx-size", 4096).effect).toBe("4,096 토큰 문맥 사용");
    expect(describeOption("--threads", 8).effect).toBe("CPU 스레드 8개 사용");
    expect(describeOption("--temp", 0.8).effect).toBe("균형 잡힌 다양성");
  });
});
