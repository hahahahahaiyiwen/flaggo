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
  security?: Array<Record<string, string[]>>;
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
    securitySchemes?: Record<string, Record<string, unknown>>;
  };
};

type JsonSchema = {
  additionalProperties?: boolean | JsonSchema;
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
  method: "get" | "post" | "put",
): Operation {
  const value = document.paths[path]?.[method];
  expect(value, `${method.toUpperCase()} ${path} must exist`).toBeDefined();
  return value!;
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

describe("OpenAPI shared-contract alignment", () => {
  const runtime = yamlDocument("flaggo-runtime-v3.yaml");
  const management = yamlDocument("flaggo-management-v3.yaml");
  const runtimeModels = jsonSchema("runtime-models-v3.schema.json");
  const managementModels = jsonSchema("management-models-v3.schema.json");
  const problemDetails = jsonSchema("problem-details-v3.schema.json");

  it("exposes only runtime decisions and health from the runtime API", () => {
    expect(Object.keys(runtime.paths)).toEqual([
      "/v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions",
      "/health/live",
      "/health/ready",
    ]);
  });

  it("exposes named DecisionContracts with immutable digest versions", () => {
    expect(management.openapi).toBe("3.1.0");
    expect(Object.keys(management.paths)).toEqual([
      "/v3/decision-contract-catalog/current",
      "/v3/decision-contracts/{contractName}",
      "/v3/decision-contracts/{contractName}/validate",
      "/v3/decision-contracts/{contractName}/versions",
      "/v3/decision-contracts/{contractName}/versions/{contractDigest}",
      "/v3/decision-contracts/{contractName}/versions/{contractDigest}/candidates",
      "/health/live",
      "/health/ready",
    ]);

    const catalog = operation(
      management,
      "/v3/decision-contract-catalog/current",
      "get",
    );
    expect(catalog.operationId).toBe("getCurrentContractCatalog");
    expect(catalog.security).toBeUndefined();
    expectParameters(catalog, [
      "#/components/parameters/IfNoneMatchHeader",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(catalog.responses["200"]?.$ref).toBe(
      "#/components/responses/CurrentContractCatalog",
    );
    expect(catalog.responses["304"]).toBeDefined();

    const validate = operation(
      management,
      "/v3/decision-contracts/{contractName}/validate",
      "post",
    );
    expect(validate.operationId).toBe("validateDecisionContract");
    expect(validate.security).toBeUndefined();
    expectParameters(validate, [
      "#/components/parameters/ContractNamePath",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(validate.requestBody?.$ref).toBe(
      "#/components/requestBodies/DecisionContract",
    );
    expectResponseSchema(
      validate,
      "200",
      "../schemas/management-models-v3.schema.json#/$defs/DecisionContractValidationResult",
    );

    const deployContract = operation(
      management,
      "/v3/decision-contracts/{contractName}",
      "put",
    );
    expect(deployContract.operationId).toBe("deployDecisionContract");
    expect(deployContract.security).toBeUndefined();
    expectParameters(deployContract, [
      "#/components/parameters/ContractNamePath",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(deployContract.requestBody?.$ref).toBe(
      "#/components/requestBodies/DecisionContract",
    );
    expect(deployContract.responses["200"]?.$ref).toBe(
      "#/components/responses/DecisionContractVersion",
    );
    expect(deployContract.responses["201"]?.$ref).toBe(
      "#/components/responses/CreatedDecisionContractVersion",
    );

    const getCurrent = operation(
      management,
      "/v3/decision-contracts/{contractName}",
      "get",
    );
    expect(getCurrent.operationId).toBe("getCurrentDecisionContract");
    expect(getCurrent.security).toBeUndefined();
    expectParameters(getCurrent, [
      "#/components/parameters/ContractNamePath",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(getCurrent.responses["200"]?.$ref).toBe(
      "#/components/responses/DecisionContractVersion",
    );

    const listVersions = operation(
      management,
      "/v3/decision-contracts/{contractName}/versions",
      "get",
    );
    expect(listVersions.operationId).toBe("listDecisionContractVersions");
    expect(listVersions.security).toBeUndefined();
    expectParameters(listVersions, [
      "#/components/parameters/ContractNamePath",
      "#/components/parameters/PageLimitQuery",
      "#/components/parameters/PageCursorQuery",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(listVersions.responses["200"]?.$ref).toBe(
      "#/components/responses/DecisionContractVersionList",
    );

    const getVersion = operation(
      management,
      "/v3/decision-contracts/{contractName}/versions/{contractDigest}",
      "get",
    );
    expect(getVersion.operationId).toBe("getDecisionContractVersion");
    expect(getVersion.security).toBeUndefined();
    expectParameters(getVersion, [
      "#/components/parameters/ContractNamePath",
      "#/components/parameters/ContractDigestPath",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(getVersion.responses["200"]?.$ref).toBe(
      "#/components/responses/DecisionContractVersion",
    );

    const submitCandidate = operation(
      management,
      "/v3/decision-contracts/{contractName}/versions/{contractDigest}/candidates",
      "post",
    );
    expect(submitCandidate.operationId).toBe("submitAnalysisCandidate");
    expect(submitCandidate.security).toBeUndefined();
    expectParameters(submitCandidate, [
      "#/components/parameters/ContractNamePath",
      "#/components/parameters/ContractDigestPath",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(submitCandidate.requestBody?.$ref).toBe(
      "#/components/requestBodies/AnalysisCandidateSubmission",
    );
    expect(submitCandidate.responses["200"]?.$ref).toBe(
      "#/components/responses/AnalysisCandidate",
    );
    expect(submitCandidate.responses["201"]?.$ref).toBe(
      "#/components/responses/CreatedAnalysisCandidate",
    );
    for (const status of ["400", "404", "409", "415", "422", "503"]) {
      expect(submitCandidate.responses[status]?.$ref).toBe(
        "#/components/responses/Problem",
      );
    }

    expect(
      management.components.requestBodies.DecisionContract!.content?.[
        "application/json"
      ]?.schema?.$ref,
    ).toBe(
      "../schemas/management-models-v3.schema.json#/$defs/DecisionContract",
    );
    for (const status of ["409", "415", "422", "503"]) {
      expect(deployContract.responses[status]?.$ref).toBe(
        "#/components/responses/Problem",
      );
    }
    expect(deployContract.responses["401"]).toBeUndefined();
    expect(deployContract.responses["403"]).toBeUndefined();

    expectRequired(managementModels.$defs!.DecisionContract!, [
      "authority",
      "name",
      "expression_syntax",
      "attributes",
      "result",
    ]);
    expectRequired(managementModels.$defs!.DecisionContractVersion!, [
      "name",
      "contractDigest",
      "status",
      "acceptedAt",
      "activeExecutableDigest",
      "contract",
    ]);
    expectRequired(managementModels.$defs!.DecisionContractVersionList!, [
      "name",
      "currentContractDigest",
      "versions",
      "nextCursor",
    ]);
    expectRequired(managementModels.$defs!.CurrentContractCatalog!, [
      "contracts",
    ]);
    expectRequired(managementModels.$defs!.AnalysisCandidateSubmission!, [
      "rules",
      "provenance",
    ]);
    expectRequired(managementModels.$defs!.AnalysisCandidateProvenance!, [
      "workspaceId",
      "cycleId",
      "attemptId",
      "evidenceCutoff",
      "evidenceWatermark",
      "analysisManifestDigest",
    ]);
    expectRequired(managementModels.$defs!.AnalysisCandidateResult!, [
      "contractName",
      "contractDigest",
      "executableDigest",
      "lifecycleState",
      "createdAt",
      "created",
    ]);
  });

  it("documents Contract Service health probes", () => {
    const live = operation(management, "/health/live", "get");
    expect(live.security).toBeUndefined();
    expectResponseSchema(
      live,
      "200",
      "../schemas/runtime-models-v3.schema.json#/$defs/LivenessResult",
    );

    const ready = operation(management, "/health/ready", "get");
    expect(ready.security).toBeUndefined();
    expectResponseSchema(
      ready,
      "200",
      "../schemas/runtime-models-v3.schema.json#/$defs/ReadinessResult",
    );
    expectResponseSchema(
      ready,
      "503",
      "../schemas/runtime-models-v3.schema.json#/$defs/ReadinessResult",
    );
    for (const status of ["200", "503"]) {
      expect(ready.responses[status]?.headers).toMatchObject({
        "X-Flaggo-Correlation-Id": {
          $ref: "#/components/headers/CorrelationId",
        },
      });
    }
  });

  it("keeps stateless runtime evaluation and retry contracts", () => {
    expect(runtime.openapi).toBe("3.1.0");
    const createDecision = operation(
      runtime,
      "/v3/decision-contracts/{contractName}/versions/{contractDigest}/decisions",
      "post",
    );

    expect(createDecision.operationId).toBe("createRuntimeDecision");
    expect(createDecision.security).toBeUndefined();
    expectParameters(createDecision, [
      "#/components/parameters/ContractNamePath",
      "#/components/parameters/ContractDigestPath",
      "#/components/parameters/CorrelationIdHeader",
    ]);
    expect(createDecision.requestBody?.required).toBe(true);
    expect(
      createDecision.requestBody?.content?.["application/json"]?.schema?.$ref,
    ).toBe(
      "../schemas/runtime-models-v3.schema.json#/$defs/RuntimeInput",
    );
    expectResponseSchema(
      createDecision,
      "200",
      "../schemas/runtime-models-v3.schema.json#/$defs/RuntimeDecision",
    );
    expect(createDecision.responses["200"]?.headers).toMatchObject({
      "X-Flaggo-Correlation-Id": {
        $ref: "#/components/headers/CorrelationId",
      },
    });
    expect(createDecision.responses["409"]).toBeUndefined();
    for (const status of ["429", "503"]) {
      expect(createDecision.responses[status]?.$ref).toBe(
        "#/components/responses/ProblemWithRetryAfter",
      );
    }
    expect(
      runtime.components.responses.ProblemWithRetryAfter!.headers,
    ).toMatchObject({
      "Retry-After": { $ref: "#/components/headers/RetryAfter" },
    });

    expect(
      runtime.components.parameters.IdempotencyKeyHeader,
    ).toBeUndefined();
    expect(runtimeModels.$defs!.RuntimeEvaluationRequest).toBeUndefined();
    expectRequired(runtimeModels.$defs!.RuntimeInput!, [
      "attributes",
    ]);
    expectRequired(runtimeModels.$defs!.RuntimeAttributes!, [
      "_random",
    ]);
    expectRequired(runtimeModels.$defs!.RuntimeDecision!, [
      "contractDigest",
      "executableDigest",
      "result",
      "evaluation",
    ]);
  });

  it("keeps SDK error classification contracts", () => {
    expect(
      runtime.components.responses.ProblemWithRetryAfter!.headers,
    ).toMatchObject({
      "X-Flaggo-Correlation-Id": {
        $ref: "#/components/headers/CorrelationId",
      },
      "Retry-After": { $ref: "#/components/headers/RetryAfter" },
    });
    expect(problemDetails.required).toBeUndefined();
    expect(problemDetails.additionalProperties).toBe(true);
    expect(Object.keys(problemDetails.properties!)).toEqual([
      "type",
      "title",
      "status",
      "detail",
      "instance",
    ]);

    expect(runtime.components.securitySchemes).toBeUndefined();
    expect(management.components.securitySchemes).toBeUndefined();
  });
});
