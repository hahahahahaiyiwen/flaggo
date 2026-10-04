import { inputError } from "./guards.js";

const maximumServiceUrlLength = 2_048;

export function normalizeServiceBaseUrl(
  value: unknown,
  path: string,
): string {
  if (typeof value !== "string" && !(value instanceof URL)) {
    return inputError(path, "The service base URL must be a string or URL.");
  }
  const source = typeof value === "string" ? value : value.toString();
  if (source.length > maximumServiceUrlLength) {
    return inputError(
      path,
      `Service base URLs cannot exceed ${maximumServiceUrlLength} characters.`,
    );
  }
  if (source.includes("?") || source.includes("#")) {
    return inputError(
      path,
      "Service base URLs cannot contain a query or fragment delimiter.",
    );
  }

  let url: URL;
  try {
    url = new URL(source);
  } catch {
    return inputError(path, "The service base URL must be absolute.");
  }
  if (
    (url.protocol !== "https:" && url.protocol !== "http:")
    || url.username !== ""
    || url.password !== ""
    || url.port === "0"
  ) {
    return inputError(
      path,
      "Service base URLs must use HTTP(S), a usable port, and no credentials.",
    );
  }
  return url.toString().replace(/\/+$/u, "");
}
