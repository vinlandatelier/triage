import { QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, RouterProvider } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { test } from "vitest";
import { ThemeProvider } from "@/components/theme-provider";
import { createQueryClient } from "@/query";
import { createAppRouter } from "@/router";

function renderAt(path: string) {
  const queryClient = createQueryClient();
  const router = createAppRouter({
    queryClient,
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <RouterProvider router={router} />
      </ThemeProvider>
    </QueryClientProvider>,
  );
}

test("renders the shell header and home page", async () => {
  renderAt("/");
  await screen.findByRole("heading", { name: "Triage" });
  screen.getByRole("link", { name: "Triage" });
  screen.getByRole("button", { name: /theme/i });
});

test("keeps the header on an unknown path", async () => {
  renderAt("/nope");
  await screen.findByRole("heading", { name: "Not found" });
  screen.getByRole("link", { name: "Triage" });
});
