import { expect, test } from "@playwright/test";

test("create a hello, then see it in the list", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("No hellos yet.")).toBeVisible();

  await page.getByLabel("Name").fill("world");
  await page.getByRole("button", { name: "Say hello" }).click();
  const list = page.getByRole("list", { name: "Recent hellos" });
  await expect(list.getByRole("listitem")).toHaveText(["world"]);
  await expect(page.getByLabel("Name")).toHaveValue("");

  await page.getByLabel("Name").fill("mars");
  await page.getByRole("button", { name: "Say hello" }).click();
  await expect(list.getByRole("listitem")).toHaveText(["mars", "world"]);

  await page.reload();
  await expect(list.getByRole("listitem")).toHaveText(["mars", "world"]);
});
