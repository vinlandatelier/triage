import { defineConfig } from "drizzle-kit";

export default defineConfig({
  dialect: "sqlite",
  schema: "./src/features/*/model.ts",
  out: "./migrations",
});
