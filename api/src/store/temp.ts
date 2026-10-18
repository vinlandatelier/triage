import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openStore, type Store } from "./open";

/** For tests: a migrated store in a fresh temp dir. `dispose` closes it and deletes the dir. */
export function openTempStore(): { readonly store: Store; readonly dispose: () => void } {
  const dir = mkdtempSync(join(tmpdir(), "triage-test-"));
  const { store } = openStore(join(dir, "triage.db"));
  return {
    store,
    dispose() {
      store.close();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}
