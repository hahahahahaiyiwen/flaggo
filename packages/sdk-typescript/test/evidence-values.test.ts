import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Ajv2020 } from "ajv/dist/2020.js";
import { describe, expect, it } from "vitest";

const root = resolve(import.meta.dirname, "../../..");
const validate = new Ajv2020({ allErrors: true, strict: true }).compile(
  JSON.parse(readFileSync(resolve(root, "contracts/schemas/evidence-value-v1.schema.json"), "utf8")) as object,
);
const corpus = JSON.parse(readFileSync(resolve(root, "contracts/fixtures/evidence-value-v1.json"), "utf8")) as {
  valid: unknown[];
  invalid: unknown[];
  invalidJsonText: { kind: "json"; value: string }[];
};

describe("canonical evidence value wire contract", () => {
  it("accepts lossless cells and rejects invalid wire values", () => {
    for (const cell of corpus.valid) expect(validate(cell), JSON.stringify(validate.errors)).toBe(true);
    for (const cell of corpus.invalid) expect(validate(cell), JSON.stringify(cell)).toBe(false);
    // JSON Schema contentMediaType is an annotation; decoding also validates the text.
    for (const cell of corpus.invalidJsonText) expect(() => JSON.parse(cell.value)).toThrow();
  });

  it("enforces signed-int64 bounds without JavaScript number coercion", () => {
    const maximum = 9223372036854775807n;
    for (let position = 0n; position < 19n; position++) {
      const step = 10n ** position;
      const prefix = maximum / step * step;
      for (const magnitude of [prefix - 1n, prefix, prefix + step - 1n, prefix + step]) {
        for (const value of [magnitude, -magnitude]) {
          expect(validate({ kind: "int64", value: value.toString() }))
            .toBe(value >= -maximum - 1n && value <= maximum);
        }
      }
    }
  });
});
