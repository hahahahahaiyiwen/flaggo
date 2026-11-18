import type { AuthorityScope } from "../configuration/types.js";
import { assertDecisionName, hasOnlyKeys, inputError, record } from "./guards.js";

export function resourceScopePath(authority: AuthorityScope): string {
  const raw = record(authority);
  if (raw === undefined || !hasOnlyKeys(raw, new Set(["tenant", "application", "environment"]))) {
    inputError("/authority", "An explicit tenant, application, and environment scope is required.");
  }
  for (const component of ["tenant", "application", "environment"] as const) {
    assertDecisionName(raw[component], `/authority/${component}`);
  }
  return `/v4/tenants/${encodeURIComponent(authority.tenant)}`
    + `/applications/${encodeURIComponent(authority.application)}`
    + `/environments/${encodeURIComponent(authority.environment)}`;
}

export function sameAuthority(left: AuthorityScope, right: AuthorityScope): boolean {
  return left.tenant === right.tenant
    && left.application === right.application
    && left.environment === right.environment;
}
