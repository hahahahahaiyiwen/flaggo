import { createRequire } from "node:module";

const packageMetadata: unknown = createRequire(import.meta.url)("../package.json");

if (
  typeof packageMetadata !== "object"
  || packageMetadata === null
  || !("version" in packageMetadata)
  || typeof packageMetadata.version !== "string"
) {
  throw new TypeError("The @flaggo/sdk package version is missing or invalid.");
}

export const SDK_VERSION = packageMetadata.version;
