/** A storage failure. Every one exits 1. */
export class StoreError extends Error {
  readonly exitCode = 1;

  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "StoreError";
  }
}

export function reason(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
