import { describe, expect, it } from "vite-plus/test";
import { providerAuthReturnUrl } from "./providerAuthReturnUrl.ts";

describe("provider auth return destinations", () => {
  it.each(["s5code", "s5code-dev", "t3code", "t3code-dev"])(
    "returns to %s Welcome and the selected settings instance",
    (scheme) => {
      expect(providerAuthReturnUrl(`${scheme}://app/welcome?code=secret#agents:machine-id`)).toBe(
        `${scheme}://app/welcome#agents:machine-id`,
      );
      expect(
        providerAuthReturnUrl(`${scheme}://app/settings/providers?instanceId=work&code=secret`),
      ).toBe(`${scheme}://app/settings/providers?instanceId=work`);
    },
  );
  it("returns to the hosted S5 client with the selected instance", () => {
    expect(
      providerAuthReturnUrl(
        "https://app.s5code.touchtech.club/settings/providers?instanceId=work&code=secret",
      ),
    ).toBe("https://app.s5code.touchtech.club/settings/providers?instanceId=work");
  });
  it.each([
    "t3code://attacker/welcome",
    "t3code://app:123/welcome",
    "t3code://app/auth/callback",
    "t3code://user@ app/welcome",
    "t3code://app/welcome/../evil",
    "https://attacker.example/welcome",
    "file:///welcome",
    "javascript:alert(1)",
  ])("rejects %s", (url) => expect(providerAuthReturnUrl(url)).toBeUndefined());
});
