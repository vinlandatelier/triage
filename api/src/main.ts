import { defineCommand, runMain } from "citty";
import packageJson from "../package.json" with { type: "json" };

const main = defineCommand({
  meta: {
    name: "triage",
    version: packageJson.version,
    description: "Local life-admin agent",
  },
});

await runMain(main);
