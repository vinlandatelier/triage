import "@/api-setup";
import { cleanup } from "@testing-library/react";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll } from "vitest";

export const server = setupServer();

// TanStack Query catches every rejected fetch, so afterEach fails the test instead of relying on the rejection.
export const unhandledRequests: string[] = [];

beforeAll(() =>
  server.listen({
    onUnhandledFrame({ frame, defaults }) {
      const { data } = frame;
      const label =
        data instanceof Object && "request" in data && data.request instanceof Request
          ? `${data.request.method} ${data.request.url}`
          : frame.protocol;
      unhandledRequests.push(label);
      defaults.error();
      // defaults.error() only logs, and returning passes the request to the real network.
      // A thrown Response.error() makes fetch reject with `fetch failed` instead.
      throw Response.error();
    },
  }),
);
// MSW records a request a macrotask after fetch() starts, so a fire-and-forget fetch needs the wait.
async function failOnUnhandled() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  const unhandled = unhandledRequests.splice(0);
  if (unhandled.length > 0)
    throw new Error(`request without an MSW handler: ${unhandled.join(", ")}`);
}

// Wait before resetHandlers(), or a request still in flight finds no handler and fails the wrong way.
afterEach(async () => {
  try {
    await failOnUnhandled();
  } finally {
    cleanup();
    server.resetHandlers();
    localStorage.clear();
    document.documentElement.className = "";
  }
});
afterAll(async () => {
  try {
    await failOnUnhandled();
  } finally {
    server.close();
  }
});

// jsdom lacks matchMedia and its scrollTo only logs "not implemented". The router scrolls on navigation;
// ThemeProvider resolves `system` with matchMedia.
window.scrollTo = () => {};
window.matchMedia = (query: string) =>
  ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  }) satisfies MediaQueryList;
