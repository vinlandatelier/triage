import { Glob } from "bun";

// hey-api copies its fetch runtime into client/ and core/ verbatim, and that code does not pass
// web's `exactOptionalPropertyTypes`. The schema-derived files stay checked.
const dir = new URL("../../web/src/api/", import.meta.url).pathname;
for await (const path of new Glob("{client,core}/**/*.ts").scan(dir)) {
  const file = Bun.file(dir + path);
  await Bun.write(file, `// @ts-nocheck\n${await file.text()}`);
}
