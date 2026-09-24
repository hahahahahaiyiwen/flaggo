import { bundleDigest, contractDigest, normalizeBundle } from "./canonical.js";
import { FlaggoHttpError, InvalidServerResponseError, RequiresApprovalError } from "./errors.js";
import { assertManifest } from "./static-schema.js";
import {
  authorization, headers, isRequiresApprovalResult, parseJson, problemOrThrow,
  verifyReceiptBindings, type CredentialProvider,
} from "./wire.js";
import type { DecisionDefinitionBundle, FetchLike, RegistrationReceipt } from "./types.js";

export interface ManifestPublicationConfig {
  controlPlaneUrl: string;
  bundle: DecisionDefinitionBundle;
  credential: CredentialProvider;
  fetch?: FetchLike;
}

export async function applyManifest(config: ManifestPublicationConfig): Promise<RegistrationReceipt> {
  assertManifest(config.bundle);
  const bundle = normalizeBundle(config.bundle);
  const digest = bundleDigest(bundle);
  const fetch = config.fetch ?? globalThis.fetch.bind(globalThis);
  const response = await fetch(
    `${config.controlPlaneUrl.replace(/\/$/, "")}/v1/definition-bundles:apply`,
    {
      method: "POST",
      headers: {
        ...Object.fromEntries(headers(await authorization(config.credential))),
        "Idempotency-Key": `flaggo:${bundle.application.id}:${bundle.application.environment}:${digest}`,
      },
      body: JSON.stringify(bundle),
    },
  );
  const body = await parseJson(response);
  if (response.status === 202) {
    if (!isRequiresApprovalResult(body, bundle, digest)) {
      throw new InvalidServerResponseError("Flaggo returned a malformed requires-approval result.");
    }
    throw new RequiresApprovalError(body);
  }
  if (!response.ok) throw new FlaggoHttpError(problemOrThrow(body, response.status));
  verifyReceiptBindings(body, bundle.application, digest,
    Object.fromEntries(Object.entries(bundle.decisions).map(([key, definition]) => [
      key, { contractDigest: contractDigest(key, definition) },
    ])));
  return body;
}

export { RequiresApprovalError } from "./errors.js";
export type { CredentialProvider } from "./wire.js";
