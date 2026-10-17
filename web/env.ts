import { defineConfig } from "@julr/vite-plugin-validate-env";
import { z } from "zod";

// Resolve against a sentinel origin instead of pattern-matching: the URL parser strips tabs and
// newlines and reads `\` as `/`, so `/\t/host` and `/\host` are protocol-relative too.
const SENTINEL = "http://sentinel.invalid";
export const isPath = (v: string) =>
  v.startsWith("/") && URL.canParse(v, SENTINEL) && new URL(v, SENTINEL).origin === SENTINEL;
const isHttpUrl = (v: string) => URL.canParse(v) && /^https?:$/.test(new URL(v).protocol);

export default defineConfig({
  validator: "standard",
  schema: {
    // The daemon's origin. Spec paths already start with `/api`, so the default is the page's own
    // origin, which both the Vite dev proxy and the release daemon serve.
    // `VITE_API_URL=` in a .env file means unset, not an empty URL.
    VITE_API_URL: z.preprocess(
      (v) => (v === "" ? undefined : v),
      z
        .string()
        .refine((v) => isPath(v) || isHttpUrl(v), "must be a path like / or an http(s) URL")
        .default("/"),
    ),
  },
});
