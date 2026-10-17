import { QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test } from "vitest";
import { ThemeProvider } from "@/components/theme-provider";
import { createQueryClient } from "@/query";
import { Route as rootRoute } from "@/routes/__root";

// Own file: addChildren replaces the generated tree's children on the shared root route.
let failures = 0;
const boom = createRoute({
  getParentRoute: () => rootRoute,
  path: "/boom",
  loader: () => {
    if (failures > 0) {
      failures -= 1;
      throw new Error("boom");
    }
  },
  component: () => <p>recovered</p>,
});

function renderBoom() {
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
}

test("a route error keeps the shell header", async () => {
  failures = 1;
  renderBoom();
  expect(await screen.findByRole("heading", { name: "Something went wrong" })).toBeDefined();
  expect(screen.getByRole("link", { name: "Triage" })).toBeDefined();
});

test("try again reruns the failed loader", async () => {
  failures = 1;
  renderBoom();
  await userEvent.click(await screen.findByRole("button", { name: "Try again" }));
  expect(await screen.findByText("recovered")).toBeDefined();
});
