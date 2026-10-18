import { QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HttpResponse } from "msw";
import { expect, test } from "vitest";
import { client } from "@/api-client";
import { ThemeProvider } from "@/components/theme-provider";
import type { Hello } from "@/features/hello/queries";
import { createQueryClient } from "@/query";
import { routeTree } from "@/routeTree.gen";
import { mock } from "@/test/handlers";
import { server } from "@/test/setup";

function renderAt(path: string) {
  const queryClient = createQueryClient();
  // The default retry waits a second, longer than `findBy` does.
  queryClient.setDefaultOptions({
    ...queryClient.getDefaultOptions(),
    queries: { ...queryClient.getDefaultOptions().queries, retry: false },
  });
  const router = createRouter({
    routeTree,
    context: { queryClient },
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

const hello = (name: string): Hello => ({
  id: `id-${name}`,
  name,
  created_at: "2026-10-17T00:00:00.000Z",
});

function names() {
  const list = screen.getByRole("list", { name: "Recent hellos" });
  return within(list)
    .getAllByRole("listitem")
    .map((item) => item.textContent);
}

test("renders the recent list", async () => {
  server.use(mock(client.api.hello.$get, () => HttpResponse.json([hello("mars"), hello("venus")])));
  renderAt("/");
  await screen.findByRole("list", { name: "Recent hellos" });
  expect(names()).toEqual(["mars", "venus"]);
});

test("submitting a name adds it at the top of the list", async () => {
  const stored = [hello("mars")];
  server.use(
    mock(client.api.hello.$get, () => HttpResponse.json(stored)),
    mock(client.api.hello.$post, async ({ request }) => {
      const created = hello((await request.json()).name);
      stored.unshift(created);
      return HttpResponse.json(created, { status: 201 });
    }),
  );
  const user = userEvent.setup();
  renderAt("/");
  await user.type(await screen.findByLabelText("Name"), "  world ");
  await user.click(screen.getByRole("button", { name: "Say hello" }));
  await screen.findByText("world");
  expect(names()).toEqual(["world", "mars"]);
  expect(screen.getByLabelText("Name")).toHaveProperty("value", "");
});

test("an empty name shows the error without a request", async () => {
  let posts = 0;
  server.use(
    mock(client.api.hello.$get, () => HttpResponse.json([])),
    mock(client.api.hello.$post, () => {
      posts += 1;
      return HttpResponse.json(hello("x"), { status: 201 });
    }),
  );
  const user = userEvent.setup();
  renderAt("/");
  await user.type(await screen.findByLabelText("Name"), "   ");
  await user.click(screen.getByRole("button", { name: "Say hello" }));
  expect(screen.getByRole("alert").textContent).toBe("name must be 1 to 80 characters");
  expect(screen.getByLabelText("Name").getAttribute("aria-invalid")).toBe("true");
  expect(posts).toBe(0);
});

test("a 422 from the server shows its field error inline", async () => {
  server.use(
    mock(client.api.hello.$get, () => HttpResponse.json([])),
    mock(client.api.hello.$post, () =>
      HttpResponse.json(
        {
          type: "https://triage.studio/problems/validation",
          title: "Validation failed",
          status: 422,
          detail: "the request is invalid",
          errors: [{ field: "name", message: "must not be empty" }],
        },
        { status: 422, headers: { "content-type": "application/problem+json" } },
      ),
    ),
  );
  const user = userEvent.setup();
  renderAt("/");
  await user.type(await screen.findByLabelText("Name"), "world");
  await user.click(screen.getByRole("button", { name: "Say hello" }));
  expect((await screen.findByRole("alert")).textContent).toBe("name must not be empty");
});

test("an unreachable API shows the route error component", async () => {
  server.use(mock(client.api.hello.$get, () => HttpResponse.error()));
  renderAt("/");
  expect(await screen.findByRole("heading", { name: "Something went wrong" })).toBeDefined();
  expect(screen.getByRole("alert").textContent).toContain("Cannot reach the triage daemon");
});
