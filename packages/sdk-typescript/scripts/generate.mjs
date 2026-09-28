import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import Ajv2020 from "ajv/dist/2020.js";
import standaloneCode from "ajv/dist/standalone/index.js";
import addFormats from "ajv-formats";
import { compileFromFile } from "json-schema-to-typescript";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(packageRoot, "..", "..");
const schemaRoot = resolve(repositoryRoot, "contracts", "schemas");
const generatedRoot = resolve(packageRoot, "src", "generated");
const checkOnly = process.argv.includes("--check");
const packageMetadata = JSON.parse(
  await readFile(resolve(packageRoot, "package.json"), "utf8"),
);

const schemas = {
  runtime: {
    source: resolve(schemaRoot, "runtime-models-v3.schema.json"),
    output: resolve(generatedRoot, "runtime-models.generated.ts"),
  },
  management: {
    source: resolve(schemaRoot, "management-models-v3.schema.json"),
    output: resolve(generatedRoot, "management-models.generated.ts"),
  },
  problem: {
    source: resolve(schemaRoot, "problem-details-v3.schema.json"),
    output: resolve(generatedRoot, "problem-details.generated.ts"),
  },
};

const banner = `/*
 * Generated from Flaggo v3 JSON Schemas. Do not edit by hand.
 * Run \`npm run generate --workspace @flaggo/sdk\` after schema changes.
 */
`;

function normalized(content) {
  return `${content.replace(/\r\n/gu, "\n").trimEnd()}\n`;
}

async function generatedTypes(source) {
  return normalized(await compileFromFile(source, {
    additionalProperties: false,
    bannerComment: banner,
    cwd: schemaRoot,
    enableConstEnums: false,
    ignoreMinAndMaxItems: true,
    strictIndexSignatures: false,
    style: {
      bracketSpacing: true,
      printWidth: 100,
      semi: true,
      singleQuote: false,
      tabWidth: 2,
      trailingComma: "all",
      useTabs: false,
    },
    unknownAny: true,
    unreachableDefinitions: true,
  }));
}

function generatedValidators(schema, validators) {
  const ajv = new Ajv2020({
    allErrors: true,
    allowUnionTypes: true,
    code: {
      esm: true,
      lines: true,
      optimize: 1,
      source: true,
    },
    strict: true,
    strictTypes: false,
  });
  addFormats(ajv);
  ajv.addSchema(schema);

  for (const schemaId of Object.values(validators)) {
    if (ajv.getSchema(schemaId) === undefined) {
      throw new Error(`Unable to compile JSON Schema '${schemaId}'.`);
    }
  }

  const source = standaloneCode(ajv, validators);
  return normalized(`${banner}${source}`);
}

function validatorDeclarations(names) {
  return normalized(`${banner}
export interface SchemaValidationError {
  readonly instancePath: string;
  readonly schemaPath: string;
  readonly keyword: string;
  readonly params: Readonly<Record<string, unknown>>;
  readonly message?: string;
}

export interface StandaloneValidator {
  (value: unknown): boolean;
  errors: readonly SchemaValidationError[] | null;
}

${names.map((name) =>
    `export const ${name}: StandaloneValidator;`).join("\n")}
`);
}

const [runtimeSchema, managementSchema, problemSchema] = await Promise.all(
  Object.values(schemas).map(async ({ source }) =>
    JSON.parse(await readFile(source, "utf8"))),
);
const runtimeValidators = {
  validateRuntimeInput:
    `${runtimeSchema.$id}#/$defs/RuntimeInput`,
  validateRuntimeDecision:
    `${runtimeSchema.$id}#/$defs/RuntimeDecision`,
};
const managementValidators = {
  validateDecisionContract:
    `${managementSchema.$id}#/$defs/DecisionContract`,
  validateDecisionContractValidationResult:
    `${managementSchema.$id}#/$defs/DecisionContractValidationResult`,
  validateDecisionContractVersion:
    `${managementSchema.$id}#/$defs/DecisionContractVersion`,
  validateDecisionContractVersionList:
    `${managementSchema.$id}#/$defs/DecisionContractVersionList`,
};
const problemValidators = {
  validateProblemDetails: problemSchema.$id,
};

const outputs = new Map([
  [schemas.runtime.output, await generatedTypes(schemas.runtime.source)],
  [schemas.management.output, await generatedTypes(schemas.management.source)],
  [schemas.problem.output, await generatedTypes(schemas.problem.source)],
  [
    resolve(generatedRoot, "runtime-validators.generated.mjs"),
    generatedValidators(runtimeSchema, runtimeValidators),
  ],
  [
    resolve(generatedRoot, "runtime-validators.generated.d.mts"),
    validatorDeclarations(Object.keys(runtimeValidators)),
  ],
  [
    resolve(generatedRoot, "management-validators.generated.mjs"),
    generatedValidators(managementSchema, managementValidators),
  ],
  [
    resolve(generatedRoot, "management-validators.generated.d.mts"),
    validatorDeclarations(Object.keys(managementValidators)),
  ],
  [
    resolve(generatedRoot, "problem-validator.generated.mjs"),
    generatedValidators(problemSchema, problemValidators),
  ],
  [
    resolve(generatedRoot, "problem-validator.generated.d.mts"),
    validatorDeclarations(Object.keys(problemValidators)),
  ],
  [
    resolve(generatedRoot, "package-version.generated.ts"),
    normalized(`${banner}
export const SDK_VERSION = ${JSON.stringify(packageMetadata.version)};
`),
  ],
]);

const stale = [];
for (const [path, content] of outputs) {
  let existing;
  try {
    existing = normalized(await readFile(path, "utf8"));
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }

  if (existing === content) continue;
  if (checkOnly) {
    stale.push(path);
    continue;
  }
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, content);
}

if (stale.length > 0) {
  console.error("Generated SDK artifacts are stale:");
  for (const path of stale) {
    console.error(`- ${path}`);
  }
  process.exitCode = 1;
}
