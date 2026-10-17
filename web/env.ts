import { defineConfig } from "@julr/vite-plugin-validate-env";
import { z } from "zod";

// Browsers read `\` as `/` in URLs, so `/\host` is protocol-relative too.
const isPath = (v: string) => v.startsWith("/") && !/^\/[/\\]/.test(v);
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
