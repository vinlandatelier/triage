const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** A ULID: 48 bits of milliseconds, then 80 random bits, in Crockford base32. */
export function newId(time: number = Date.now()): string {
  let head = "";
  let rest = time;
  for (let i = 0; i < 10; i++) {
    head = `${CROCKFORD[rest % 32]}${head}`;
    rest = Math.floor(rest / 32);
  }
  // 16 characters of 5 bits each; masking a random byte to 5 bits keeps the draw uniform.
  const random = crypto.getRandomValues(new Uint8Array(16));
  let tail = "";
  for (const byte of random) {
    tail += CROCKFORD[byte & 31];
  }
  return head + tail;
}
