import { client } from "@/api/client.gen";
import { toProblemError } from "@/api-config";

// Not in api-config.ts: the generated client imports that file while it loads.
client.interceptors.error.use(toProblemError);
