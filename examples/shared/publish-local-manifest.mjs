import {
  applyManifest,
  RequiresApprovalError,
} from "../../packages/sdk-typescript/dist/management.js";

export async function publishLocalManifest({
  controlPlaneUrl,
  bundle,
  fetch: fetchImpl = globalThis.fetch,
}) {
  const configuration = {
    controlPlaneUrl,
    bundle,
    credential: { mode: "local-development" },
    fetch: fetchImpl,
  };
  try {
    return { receipt: await applyManifest(configuration), approvalRequired: false };
  } catch (error) {
    if (!(error instanceof RequiresApprovalError)) throw error;
    // Only this trusted local fixture harness grants approval, never the runtime client.
    const response = await fetchImpl(
      `${controlPlaneUrl.replace(/\/$/, "")}/v1/definition-bundle-approvals/${encodeURIComponent(error.approvalRequestId)}:approve`,
      {
        method: "POST",
        headers: {
          Authorization: "Flaggo-Local-Development",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          expectedBundleDigest: error.bundleDigest,
          comment: "Explicit trusted local example bootstrap.",
        }),
      },
    );
    const approval = await response.json();
    if (!response.ok || approval.status !== "approved") {
      throw new Error(
        `Local manifest approval failed with HTTP ${response.status}: ${JSON.stringify(approval)}`,
      );
    }
    return {
      receipt: await applyManifest(configuration),
      approvalRequired: true,
      approvalRequestId: error.approvalRequestId,
    };
  }
}
