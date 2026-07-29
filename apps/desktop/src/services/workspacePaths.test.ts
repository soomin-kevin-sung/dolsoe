import { describe, expect, it } from "vitest";

import { workspaceDisplayName } from "./workspacePaths";

describe("workspaceDisplayName", () => {
  it("uses the final directory on Windows and macOS paths", () => {
    expect(workspaceDisplayName("C:\\Users\\tester\\Documents")).toBe("Documents");
    expect(workspaceDisplayName("/Users/tester/Documents/")).toBe("Documents");
  });

  it("keeps a root path readable", () => {
    expect(workspaceDisplayName("/")).toBe("/");
    expect(workspaceDisplayName("C:\\")).toBe("C:");
  });
});
