import "./index.css";
import "@/api-setup";
import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ThemeProvider } from "@/components/theme-provider";
import { createQueryClient } from "@/query";
import { createAppRouter } from "@/router";

// A deploy replaces the hashed chunks an open tab still points at. Reload once to pick up the
// new build; the timestamp stops a loop when the chunk is missing for another reason.
const PRELOAD_RELOAD_KEY = "triage-preload-reload";
window.addEventListener("vite:preloadError", (event) => {
  const last = Number(sessionStorage.getItem(PRELOAD_RELOAD_KEY) ?? 0);
  if (Date.now() - last < 10_000) return;
  event.preventDefault();
  sessionStorage.setItem(PRELOAD_RELOAD_KEY, String(Date.now()));
  window.location.reload();
});

const queryClient = createQueryClient();
const router = createAppRouter({ queryClient });

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from index.html");

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <RouterProvider router={router} />
      </ThemeProvider>
    </QueryClientProvider>
  </StrictMode>,
);
