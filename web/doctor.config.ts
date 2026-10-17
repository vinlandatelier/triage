import type { ReactDoctorConfig } from "react-doctor/api";

// Rules run at their recommended defaults; tanstack-query rules switch on by themselves once
// @tanstack/react-query is a dependency.
export default {
  // One network request per dependency would make every run depend on Socket.dev; renovate covers updates.
  supplyChain: { enabled: false },
  ignore: { files: ["src/api/**"] },
} satisfies ReactDoctorConfig;
