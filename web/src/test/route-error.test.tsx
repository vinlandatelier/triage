import { QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { ThemeProvider } from "@/components/theme-provider";
import { createQueryClient } from "@/query";
import { Route as rootRoute } from "@/routes/__root";

// Own file: addChildren replaces the generated tree's children on the shared root route.
const boom = createRoute({
  getParentRoute: () => rootRoute,
  path: "/boom",
  loader: () => {
    throw new Error("boom");
  },
});

test("a route error keeps the shell header", async () => {
  const queryClient = createQueryClient();
  const router = createRouter({
    routeTree: rootRoute.addChildren([boom]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: ["/boom"] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <RouterProvider router={router} />
      </ThemeProvider>
    </QueryClientProvider>,
  );
  expect(await screen.findByRole("heading", { name: "Something went wrong" })).toBeDefined();
  expect(screen.getByRole("link", { name: "Triage" })).toBeDefined();
});
