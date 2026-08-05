import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { parse } from "yaml";

type Reference = { $ref: string };

type MediaType = {
  schema: Reference;
};

type RequestBody = {
  required?: boolean;
  content?: Record<string, MediaType>;
  $ref?: string;
};

type Response = {
  $ref?: string;
  headers?: Record<string, Reference>;
  content?: Record<string, MediaType>;
};

type Operation = {
  operationId: string;
  security: Array<Record<string, string[]>>;
  parameters?: Reference[];
  requestBody?: RequestBody;
  responses: Record<string, Response>;
};

type OpenApiDocument = {
  openapi: string;
  paths: Record<string, Record<string, Operation>>;
  components: {
    parameters: Record<string, Record<string, unknown>>;
    requestBodies: Record<string, RequestBody>;
    responses: Record<string, Response>;
    headers: Record<string, Record<string, unknown>>;
    securitySchemes: Record<string, Record<string, unknown>>;
  };
};

type JsonSchema = {
  required?: string[];
  properties?: Record<string, unknown>;
  allOf?: JsonSchema[];
  $defs?: Record<string, JsonSchema>;
};

const repositoryRoot = resolve(import.meta.dirname, "../../..");

function yamlDocument(file: string): OpenApiDocument {
  return parse(
    readFileSync(resolve(repositoryRoot, "contracts/openapi", file), "utf8"),
  ) as OpenApiDocument;
}

function jsonSchema(file: string): JsonSchema {
  return JSON.parse(
    readFileSync(resolve(repositoryRoot, "contracts/schemas", file), "utf8"),
  ) as JsonSchema;
}

function operation(
  document: OpenApiDocument,
  path: string,
  method: "get" | "post",
): Operation {
  const value = document.paths[path]?.[method];
  expect(value, `${method.toUpperCase()} ${path} must exist`).toBeDefined();
  return value!;
}

function expectSecurity(operationValue: Operation, scope: string): void {
  expect(operationValue.security).toEqual([{ oauth2: [scope] }]);
}

function expectParameters(
  operationValue: Operation,
  references: string[],
): void {
  expect(operationValue.parameters?.map((parameter) => parameter.$ref)).toEqual(
    references,
  );
}

function expectResponseSchema(
  operationValue: Operation,
  status: string,
  reference: string,
): void {
  expect(
    operationValue.responses[status]?.content?.["application/json"]?.schema
      ?.$ref,
  ).toBe(reference);
}

function expectRequired(schema: JsonSchema, required: string[]): void {
  expect(schema.required).toEqual(expect.arrayContaining(required));
}

describe("frozen OpenAPI SDK compatibility", () => {
  const runtime = yamlDocument("flaggo-runtime-v1.yaml");
  const management = yamlDocument("flaggo-management-v1.yaml");
  const runtimeModels = jsonSchema("runtime-models-v1.schema.json");
  const managementModels = jsonSchema("management-models-v1.schema.json");
  const definitionBundle = jsonSchema(
    "decision-definition-bundle-v1.schema.json",
  );
  const problemDetails = jsonSchema("problem-details-v1.schema.json");

  it("keeps the startup apply operation and typed approval response", () => {
    expect(management.openapi).toBe("3.1.0");
    const apply = operation(
      management,
      "/v1/definition-bundles:apply",
      "post",
    );

    expect(apply.operationId).toBe("applyDefinitionBundle");
    expectSecurity(apply, "polari.definitions:apply");
    expectParameters(apply, [
      "#/components/parameters/ApplyIdempotencyKeyHeader",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(apply.requestBody?.$ref).toBe("#/components/requestBodies/Bundle");
    expect(
      management.components.requestBodies.Bundle!.content?.["application/json"]
        ?.schema?.$ref,
    ).toBe("../schemas/decision-definition-bundle-v1.schema.json");
    expectResponseSchema(
      apply,
      "200",
      "../schemas/management-models-v1.schema.json#/$defs/RegistrationReceipt",
    );
    expectResponseSchema(
      apply,
      "202",
      "../schemas/management-models-v1.schema.json#/$defs/RequiresApprovalResult",
    );
    for (const status of ["401", "403", "409", "415", "422"]) {
      expect(apply.responses[status]?.$ref).toBe(
        "#/components/responses/Problem",
      );
    }

    expect(
      management.components.parameters.ApplyIdempotencyKeyHeader!.name,
    ).toBe("Idempotency-Key");
    expect(
      management.components.parameters.ApplyIdempotencyKeyHeader!.required,
    ).toBe(true);
    expectRequired(definitionBundle, [
      "format",
      "application",
      "source",
      "definitions",
    ]);
    expectRequired(managementModels.$defs!.RegistrationReceipt!, [
      "application",
      "environment",
      "bundleDigest",
      "acceptedDefinitions",
      "compatibility",
      "status",
      "issues",
    ]);
    expectRequired(managementModels.$defs!.RequiresApprovalResult!, [
      "status",
      "approvalRequestId",
      "application",
      "environment",
      "bundleDigest",
      "expiresAt",
      "snapshotUrl",
      "changes",
      "issues",
    ]);
  });

  it("keeps decide identity, idempotency, retry, and response contracts", () => {
    expect(runtime.openapi).toBe("3.1.0");
    const decide = operation(
      runtime,
      "/v1/decisions/{decisionKey}:decide",
      "post",
    );

    expect(decide.operationId).toBe("decide");
    expectSecurity(decide, "polari.decisions:decide");
    expectParameters(decide, [
      "#/components/parameters/DecisionKeyPath",
      "#/components/parameters/IdempotencyKeyHeader",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(decide.requestBody?.required).toBe(true);
    expect(
      decide.requestBody?.content?.["application/json"]?.schema?.$ref,
    ).toBe("../schemas/runtime-models-v1.schema.json#/$defs/DecideRequest");
    expectResponseSchema(
      decide,
      "200",
      "../schemas/runtime-models-v1.schema.json#/$defs/ServerDecisionResult",
    );
    expect(decide.responses["200"]?.headers).toMatchObject({
      "X-Flaggo-Correlation-Id": {
        $ref: "#/components/headers/CorrelationId",
      },
      "Idempotency-Key-Expires-At": {
        $ref: "#/components/headers/IdempotencyKeyExpiresAt",
      },
    });
    for (const status of ["409", "429", "503"]) {
      expect(decide.responses[status]?.$ref).toBe(
        "#/components/responses/ProblemWithRetryAfter",
      );
    }
    expect(
      runtime.components.responses.ProblemWithRetryAfter!.headers,
    ).toMatchObject({
      "Retry-After": { $ref: "#/components/headers/RetryAfter" },
    });

    expect(runtime.components.parameters.IdempotencyKeyHeader!.name).toBe(
      "Idempotency-Key",
    );
    expect(runtime.components.parameters.IdempotencyKeyHeader!.required).toBe(
      false,
    );
    expectRequired(runtimeModels.$defs!.DecideRequest!, [
      "expectedContract",
      "runtimeContext",
      "client",
    ]);
    expectRequired(runtimeModels.$defs!.ServerDecisionResult!.allOf![0]!, [
      "decisionKey",
      "definition",
      "decisionId",
      "decisionMode",
      "confidence",
      "fallback",
      "policy",
      "definitionStatus",
      "exposure",
      "reason",
      "auditId",
    ]);
  });

  it("keeps exposure confirmation and SDK error classification contracts", () => {
    const confirm = operation(
      runtime,
      "/v1/exposures/{decisionId}:confirm",
      "post",
    );

    expect(confirm.operationId).toBe("confirmExposure");
    expectSecurity(confirm, "polari.exposures:confirm");
    expectParameters(confirm, [
      "#/components/parameters/DecisionIdPath",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(
      confirm.requestBody?.content?.["application/json"]?.schema?.$ref,
    ).toBe(
      "../schemas/runtime-models-v1.schema.json#/$defs/ExposureConfirmationRequest",
    );
    expectResponseSchema(
      confirm,
      "200",
      "../schemas/runtime-models-v1.schema.json#/$defs/ExposureConfirmationResult",
    );
    expectRequired(runtimeModels.$defs!.ExposureConfirmationRequest!, [
      "confirmToken",
    ]);
    expectRequired(runtimeModels.$defs!.ExposureConfirmationResult!, [
      "exposureId",
      "decisionId",
      "status",
      "confirmedAt",
    ]);
    expectRequired(problemDetails, ["type", "status", "code"]);
    expect(Object.keys(problemDetails.properties!)).toEqual(
      expect.arrayContaining([
        "type",
        "status",
        "code",
      "title",
      "detail",
      "instance",
      "correlationId",
        "retryAfterSeconds",
        "clientFallback",
      ]),
    );

    const securityText = JSON.stringify({
      runtime: runtime.components.securitySchemes.oauth2,
      management: management.components.securitySchemes.oauth2,
    });
    expect(securityText).toContain("polari.decisions:decide");
    expect(securityText).toContain("polari.exposures:confirm");
    expect(securityText).toContain("polari.definitions:apply");
  });
});
