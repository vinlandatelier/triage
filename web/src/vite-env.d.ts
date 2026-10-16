/// <reference types="vite/client" />

type ImportMetaEnvAugmented = import("@julr/vite-plugin-validate-env").ImportMetaEnvAugmented<
  typeof import("../env").default
>;

interface ViteTypeOptions {
  strictImportMetaEnv: unknown;
}

interface ImportMetaEnv extends ImportMetaEnvAugmented {}
