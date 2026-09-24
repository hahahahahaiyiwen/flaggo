import * as ts from "typescript";

import { bundleDigest, contractDigest, normalizeBundle } from "./canonical.js";
import { assertManifest, ManifestValidationError } from "./static-schema.js";
import type { DecisionDefinitionBundle, RuntimeCatalog } from "./types.js";

function decimalIdentity(token: string): string {
  const match = /^(-?)(\d+)(?:\.(\d+))?(?:e([+-]?\d+))?$/iu.exec(token);
  if (match === null) throw new ManifestValidationError("/", "Invalid JSON number.");
  let digits = `${match[2]}${match[3] ?? ""}`.replace(/^0+/u, "");
  if (digits.length === 0) return "0";
  const significant = digits.replace(/0+$/u, "");
  const exponent = BigInt(match[4] ?? "0") - BigInt((match[3] ?? "").length)
    + BigInt(digits.length - significant.length);
  digits = significant;
  return `${match[1]}${digits}e${exponent}`;
}

export function parseManifest(text: string): DecisionDefinitionBundle {
  const value: unknown = JSON.parse(text);
  const syntax = ts.parseJsonText("manifest.json", text);
  function visit(node: ts.Node): void {
    if (ts.isObjectLiteralExpression(node)) {
      const keys = new Set<string>();
      for (const property of node.properties) {
        if (!ts.isPropertyAssignment(property) || !ts.isStringLiteral(property.name)) {
          throw new ManifestValidationError("/", "Only JSON object properties are supported.");
        }
        if (keys.has(property.name.text)) {
          throw new ManifestValidationError("/", `Duplicate JSON property: ${property.name.text}.`);
        }
        keys.add(property.name.text);
      }
    }
    if (ts.isNumericLiteral(node)
      || (ts.isPrefixUnaryExpression(node) && ts.isNumericLiteral(node.operand))) {
      const token = node.getText(syntax);
      const number = Number(token);
      if (!Number.isFinite(number) || decimalIdentity(token) !== decimalIdentity(JSON.stringify(number))) {
        throw new ManifestValidationError("/", "Numbers must round-trip through the canonical IEEE-754 representation.");
      }
      return;
    }
    ts.forEachChild(node, visit);
  }
  visit(syntax);
  assertManifest(value);
  return value;
}

function literal(value: unknown): ts.Expression {
  if (value === null) return ts.factory.createNull();
  if (typeof value === "string") return ts.factory.createStringLiteral(value);
  if (typeof value === "boolean") return value ? ts.factory.createTrue() : ts.factory.createFalse();
  if (typeof value === "number") {
    return value < 0
      ? ts.factory.createPrefixUnaryExpression(ts.SyntaxKind.MinusToken, ts.factory.createNumericLiteral(-value))
      : ts.factory.createNumericLiteral(value);
  }
  if (Array.isArray(value)) return ts.factory.createArrayLiteralExpression(value.map(literal));
  if (typeof value === "object" && value !== null) {
    return ts.factory.createObjectLiteralExpression(Object.entries(value).map(([key, item]) =>
      ts.factory.createPropertyAssignment(
        key === "__proto__"
          ? ts.factory.createComputedPropertyName(ts.factory.createStringLiteral(key))
          : ts.factory.createStringLiteral(key),
        literal(item),
      )
    ), true);
  }
  throw new ManifestValidationError("/", "Generated catalogs must contain JSON values.");
}

export function compileManifest(manifest: DecisionDefinitionBundle): {
  bundle: DecisionDefinitionBundle;
  catalog: RuntimeCatalog;
  catalogSource: string;
} {
  assertManifest(manifest);
  const bundle = normalizeBundle(manifest);
  const catalog: RuntimeCatalog = {
    format: "flaggo.runtime-catalog/v1",
    application: structuredClone(bundle.application),
    bundleDigest: bundleDigest(bundle),
    decisions: Object.fromEntries(Object.entries(bundle.decisions).map(([key, definition]) => [
      key,
      {
        result: definition.result,
        context: definition.context ?? {},
        inputs: definition.inputs ?? {},
        contractDigest: contractDigest(key, definition),
      },
    ])),
  };
  const file = ts.createSourceFile("flaggo.generated.ts", "", ts.ScriptTarget.ES2022, false, ts.ScriptKind.TS);
  const expression = ts.factory.createSatisfiesExpression(
    ts.factory.createAsExpression(literal(catalog), ts.factory.createTypeReferenceNode("const")),
    ts.factory.createTypeReferenceNode("RuntimeCatalog"),
  );
  const declaration = ts.factory.createVariableStatement(
    [ts.factory.createModifier(ts.SyntaxKind.ExportKeyword)],
    ts.factory.createVariableDeclarationList([
      ts.factory.createVariableDeclaration("catalog", undefined, undefined, expression),
    ], ts.NodeFlags.Const),
  );
  const source = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed })
    .printNode(ts.EmitHint.Unspecified, declaration, file);
  return {
    bundle,
    catalog,
    catalogSource: `// Generated from the decision manifest. Do not edit.\nimport type { RuntimeCatalog } from "@flaggo/sdk";\n\n${source}\n`,
  };
}

export { bundleDigest, contractDigest, normalizeBundle } from "./canonical.js";
export { assertManifest, ManifestValidationError } from "./static-schema.js";
