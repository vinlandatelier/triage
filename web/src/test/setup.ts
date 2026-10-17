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
      unhandledRequests.push(
        data instanceof Object && "request" in data && data.request instanceof Request
          ? `${data.request.method} ${data.request.url}`
          : frame.protocol,
      );
      defaults.error();
    },
  }),
);
afterEach(() => {
  cleanup();
  server.resetHandlers();
  localStorage.clear();
  document.documentElement.className = "";
  const unhandled = unhandledRequests.splice(0);
  if (unhandled.length > 0)
    throw new Error(`request without an MSW handler: ${unhandled.join(", ")}`);
});
afterAll(() => server.close());

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
