import { CliError, run } from "./cli";
import { ConfigError } from "./config";
import { StoreError } from "./store/error";

try {
  await run(Bun.argv.slice(2));
} catch (err) {
  if (!(err instanceof ConfigError || err instanceof CliError || err instanceof StoreError)) {
    throw err;
  }
  // No logger exists yet when config fails.
  process.stderr.write(`error: ${err.message}\n`);
  process.exitCode = err.exitCode;
}
