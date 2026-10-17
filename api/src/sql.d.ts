// Bun imports any file `with { type: "text" }`; bun-types declares only a few extensions.
declare module "*.sql" {
  const text: string;
  export default text;
}
