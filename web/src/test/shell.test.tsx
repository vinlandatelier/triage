import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { ThemeProvider } from "@/components/theme-provider";
import { routeTree } from "@/routeTree.gen";

function renderAt(path: string) {
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <ThemeProvider>
      <RouterProvider router={router} />
    </ThemeProvider>,
  );
}

test("renders the shell header and home page", async () => {
  renderAt("/");
  expect(await screen.findByRole("heading", { name: "Triage" })).toBeDefined();
  expect(screen.getByRole("link", { name: "Triage" })).toBeDefined();
  expect(screen.getByRole("button", { name: /theme/i })).toBeDefined();
});

test("renders not found for an unknown path", async () => {
  renderAt("/nope");
  expect(await screen.findByRole("heading", { name: "Not found" })).toBeDefined();
});

test("fails a request that has no handler", async () => {
  await expect(fetch("http://localhost/api/unmocked")).rejects.toMatchObject({
    cause: { message: expect.stringContaining('"error" strategy for the "onUnhandledFrame"') },
  });
});
