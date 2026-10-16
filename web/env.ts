import { defineConfig } from "@julr/vite-plugin-validate-env";
import { z } from "zod";

const isPath = (v: string) => v.startsWith("/") && !v.startsWith("//");
const isHttpUrl = (v: string) => URL.canParse(v) && /^https?:$/.test(new URL(v).protocol);

export default defineConfig({
  validator: "standard",
  schema: {
    // A path stays same-origin, so the Vite dev proxy and the release daemon both serve it.
    VITE_API_URL: z
      .string()
      .refine((v) => isPath(v) || isHttpUrl(v), "must be a path like /api or an http(s) URL")
      .default("/api"),
  },
});
