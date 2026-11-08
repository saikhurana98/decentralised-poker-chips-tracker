import { test, expect } from "@playwright/test";
test("public lobby, mobile join, host approval, betting and settlement", async ({
  browser,
  page: host,
}) => {
  const errors: string[] = [];
  host.on("pageerror", (error) => errors.push(error.message));
  host.on("dialog", (dialog) => {
    void dialog.accept(dialog.type() === "prompt" ? "100" : undefined);
  });
  const mobile = await browser.newContext({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  });
  try {
    const guest = await mobile.newPage();
    guest.on("pageerror", (error) => errors.push(error.message));
    await guest.goto("http://localhost:18083");
    await host.goto("/");
    await host.locator("[name=name]").fill("Host");
    await host.locator("[name=title]").fill("Sai’s Table");
    await host.getByRole("button", { name: "Create your table" }).click();
    await expect(host.locator(".bar .code")).toBeVisible();
    const code = await host.locator(".bar .code").textContent();
    if (!code) throw Error("Missing room ID");
    const room = guest.locator(".active-game").filter({ hasText: code });
    await expect(room).toContainText("Sai’s Table");
    await room.getByRole("button", { name: "Join" }).click();
    await expect(guest.locator("[name=code]")).toHaveValue(code);
    await guest.locator("[name=name]").fill("Guest");
    await guest.getByRole("button", { name: "Request a seat" }).click();
    await expect(guest.getByText("Your seat is waiting")).toBeVisible();
    await expect(host.locator("[data-approve]")).toHaveCount(2);
    await host.locator("[data-approve]").first().click();
    await expect(host.locator(".seat:not(.empty)")).toHaveCount(1);
    await host.locator("[data-approve]").first().click();
    await expect(host.locator(".seat:not(.empty)")).toHaveCount(2);
    await host.getByRole("button", { name: "Start first hand" }).click();
    await host.locator("[data-action=call]").click();
    await expect(guest.locator(".your-controls")).toBeVisible();
    const controls = await guest.locator(".your-controls").boundingBox();
    expect(controls?.y).toBeGreaterThanOrEqual(0);
    expect((controls?.y ?? 0) + (controls?.height ?? 0)).toBeLessThanOrEqual(
      844,
    );
    await guest.locator("[data-action=fold]").click();
    await expect(host.getByRole("button", { name: "Next hand" })).toBeVisible();
    await guest.reload();
    await expect(guest.locator("#connection")).toContainText("Live");
    expect(
      await guest.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await host.locator("#invite").click();
    await expect(host.locator("#qr")).toBeVisible();
    await expect
      .poll(() =>
        host
          .locator("#qr")
          .evaluate((image: HTMLImageElement) => image.naturalWidth),
      )
      .toBeGreaterThan(0);
    await host.locator("#close-share").click();
    await host.getByText("Table management", { exact: true }).click();
    await host.locator("[data-command=end]").click();
    await expect(host.locator(".center")).toContainText("Session complete");
    expect(errors).toEqual([]);
  } finally {
    await mobile.close();
  }
});
