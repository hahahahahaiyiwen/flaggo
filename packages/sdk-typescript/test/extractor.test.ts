import {
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { runExtractorCli } from "../src/extract-cli.js";
import {
  bundleDigest,
} from "../src/index.js";
import {
  StaticExtractionError,
  extractDecisionBundle,
} from "../src/extractor.js";

const temporaryDirectories: string[] = [];

function sourceFile(source: string): { fileName: string; rootDir: string } {
  const rootDir = mkdtempSync(join(tmpdir(), "flaggo-extract-"));
  temporaryDirectories.push(rootDir);
  const fileName = join(rootDir, "app.ts");
  writeFileSync(
    fileName,
    `
      import {
        createDerivedMetricHandle,
        createInferenceSignalHandle,
        createSignalHandle
      } from "@flaggo/sdk";
      ${source}
    `,
    "utf8",
  );
  return { fileName, rootDir };
}

function numberDefinition(defaultValue = 800): string {
  return `{
    key: "tetris.dropInterval",
    valueType: "number",
    actionSpace: {
      type: "number",
      min: 200,
      max: 1500,
      step: 50,
      default: ${defaultValue}
    },
    fallback: { value: ${defaultValue}, reason: "declared-default" },
    intent: { type: "natural-language", text: "Keep play responsive." },
    policy: { kind: "inline", constraints: [] }
  }`;
}

function cooldownDefinition(seconds: string): string {
  return numberDefinition().replace(
    "constraints: []",
    `constraints: [{ kind: "cooldown", seconds: ${seconds} }]`,
  );
}

function extract(source: string) {
  const { fileName, rootDir } = sourceFile(source);
  return extractDecisionBundle({
    fileNames: [fileName],
    rootDir,
    application: { id: "tetris-demo", environment: "dev" },
    source: { repository: "flaggo", path: "apps/tetris" },
    compilerOptions: { noResolve: true },
  });
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe("static decision extraction", () => {
  it("emits deterministic descriptors and deduplicates identical definitions", () => {
    const definition = numberDefinition();
    const result = extract(`
      createInferenceSignalHandle({
        kind: "metric",
        key: "tetris.boardPressure",
        type: "number",
        source: "app-emitted"
      }, sink);
      flaggo.tune.number("tetris.dropInterval", {
        definition: ${definition},
        context: { sessionId }
      });
      flaggo.tune.number("tetris.dropInterval", {
        definition: (${definition} as const),
        context: { sessionId: anotherSession }
      });
    `);

    expect(result).toMatchObject({
      format: "flaggo.static-extraction/v1",
      bundle: {
        format: "flaggo.decision-definition-bundle/v1",
        application: { id: "tetris-demo", environment: "dev" },
      },
      descriptors: [
        { decisionKey: "tetris.dropInterval", sourceFile: "app.ts" },
        { decisionKey: "tetris.dropInterval", sourceFile: "app.ts" },
      ],
    });
    expect(result.bundle.definitions).toHaveLength(1);
    expect(result.bundle.signals).toEqual([
      expect.objectContaining({ key: "tetris.boardPressure" }),
    ]);
    expect(result.bundleDigest).toBe(bundleDigest(result.bundle));
    expect(result.descriptors[0]!.contractDigest).toBe(
      result.descriptors[1]!.contractDigest,
    );
    expect(extract(`
      flaggo.tune.number("tetris.dropInterval", {
        definition: ${definition},
        context: { sessionId }
      });
    `).bundle).toEqual(
      expect.objectContaining({
        definitions: result.bundle.definitions,
      }),
    );
  });

  it("fails closed for dynamic keys and unsupported static syntax", () => {
    const dynamicKey = () =>
      extract(`
        const key = "tetris.dropInterval";
        flaggo.tune.number(key, {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const spreadDefinition = () =>
      extract(`
        const shared = { valueType: "number" };
        flaggo.tune.number("tetris.dropInterval", {
          definition: { ...shared, ${numberDefinition().slice(1)},
          context: {}
        });
      `);
    const computedTuneAccess = () =>
      extract(`
        flaggo.tune["number"]("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const conditionalCall = () =>
      extract(`
        if (enabled) {
          flaggo.tune.number("tetris.dropInterval", {
            definition: ${numberDefinition()},
            context: {}
          });
        }
      `);
    const dynamicComputedTuneAccess = () =>
      extract(`
        const method = "number";
        flaggo.tune[method]("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const computedTuneReceiver = () =>
      extract(`
        flaggo["tune"].number("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const aliasedTuneMethod = () =>
      extract(`
        flaggo.tune.number("valid.decision", {
          definition: ${numberDefinition().replaceAll(
            "tetris.dropInterval",
            "valid.decision",
          )},
          context: {}
        });
        const tuneNumber = flaggo.tune.number;
        tuneNumber("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const aliasedTuneObject = () =>
      extract(`
        flaggo.tune.number("valid.decision", {
          definition: ${numberDefinition().replaceAll(
            "tetris.dropInterval",
            "valid.decision",
          )},
          context: {}
        });
        const tune = flaggo.tune;
        tune.number("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const destructuredTuneMethod = () =>
      extract(`
        flaggo.tune.number("valid.decision", {
          definition: ${numberDefinition().replaceAll(
            "tetris.dropInterval",
            "valid.decision",
          )},
          context: {}
        });
        const { number: tuneNumber } = flaggo.tune;
        tuneNumber("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const destructuredTuneObject = () =>
      extract(`
        flaggo.tune.number("valid.decision", {
          definition: ${numberDefinition().replaceAll(
            "tetris.dropInterval",
            "valid.decision",
          )},
          context: {}
        });
        const { tune } = flaggo;
        tune.number("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const aliasedRootClient = () =>
      extract(`
        flaggo.tune.number("valid.decision", {
          definition: ${numberDefinition().replaceAll(
            "tetris.dropInterval",
            "valid.decision",
          )},
          context: {}
        });
        const fg = flaggo;
        fg.tune.number("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);
    const importedRootAlias = () =>
      extract(`
        import { flaggo as fg } from "./client.js";
        flaggo.tune.number("valid.decision", {
          definition: ${numberDefinition().replaceAll(
            "tetris.dropInterval",
            "valid.decision",
          )},
          context: {}
        });
        fg.tune.number("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `);

    expect(dynamicKey).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "static-key-required",
      }),
    );
    expect(spreadDefinition).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(computedTuneAccess).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(conditionalCall).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(dynamicComputedTuneAccess).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(computedTuneReceiver).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(aliasedTuneMethod).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(aliasedTuneObject).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(destructuredTuneMethod).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(destructuredTuneObject).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(aliasedRootClient).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
    expect(importedRootAlias).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "unsupported-static-syntax",
      }),
    );
  });

  it("ignores unrelated APIs that happen to use tune.number", () => {
    const result = extract(`
      unrelated.tune.number("unrelated.decision", {
        definition: {
          ${numberDefinition().slice(1, -1).replaceAll(
            "tetris.dropInterval",
            "unrelated.decision",
          )}
        },
        context: {}
      });
      flaggo["anything"].number("other.decision", {
        definition: {
          ${numberDefinition().slice(1, -1).replaceAll(
            "tetris.dropInterval",
            "other.decision",
          )}
        },
        context: {}
      });
      const otherApi = flaggo["anything"];
      otherApi.number("aliased.other.decision", {
        definition: {
          ${numberDefinition().slice(1, -1).replaceAll(
            "tetris.dropInterval",
            "aliased.other.decision",
          )}
        },
        context: {}
      });
      flaggo.tune.number("tetris.dropInterval", {
        definition: ${numberDefinition()},
        context: {}
      });
    `);

    expect(result.bundle.definitions.map(({ key }) => key)).toEqual([
      "tetris.dropInterval",
    ]);
  });

  it("ignores unrelated APIs with signal-factory method names", () => {
    const result = extract(`
      unrelated.createSignalHandle({ arbitrary: "payload" }, sink);
      unrelated.createInferenceSignalHandle({ arbitrary: "payload" }, sink);
      unrelated.createDerivedMetricHandle({ arbitrary: "payload" });
      flaggo.tune.number("tetris.dropInterval", {
        definition: ${numberDefinition()},
        context: {}
      });
    `);

    expect(result.bundle.signals).toBeUndefined();
    expect(result.bundle.definitions).toHaveLength(1);
  });

  it("rejects conflicting definitions for one decision key", () => {
    expect(() =>
      extract(`
        flaggo.tune.number("tetris.dropInterval", {
          definition: ${numberDefinition(800)},
          context: {}
        });
        flaggo.tune.number("tetris.dropInterval", {
          definition: ${numberDefinition(850)},
          context: {}
        });
      `)
    ).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "contract-conflict",
      }),
    );
  });

  it("rejects definitions that violate the frozen bundle schema", () => {
    const invalidDefinition = numberDefinition().replace(
      'intent: { type: "natural-language", text: "Keep play responsive." }',
      'intent: { type: "metric-objective" }',
    );
    expect(() =>
      extract(`
        flaggo.tune.number("tetris.dropInterval", {
          definition: ${invalidDefinition},
          context: {}
        });
      `)
    ).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "invalid-static-definition",
      }),
    );
  });

  it("accepts frozen-schema definitions without optional intent", () => {
    const definition = numberDefinition().replace(
      'intent: { type: "natural-language", text: "Keep play responsive." },',
      "",
    );

    expect(extract(`
      flaggo.tune.number("tetris.dropInterval", {
        definition: ${definition},
        context: {}
      });
    `).bundle.definitions).toHaveLength(1);
  });

  it("accepts huge finite cooldowns from the frozen v1 contract", () => {
    const definition = extract(`
      flaggo.tune.number("tetris.dropInterval", {
        definition: ${cooldownDefinition("1.7976931348623157e308")},
        context: {}
      });
    `).bundle.definitions[0]!;

    expect(definition.policy).toMatchObject({
      constraints: [
        { kind: "cooldown", seconds: Number.MAX_VALUE },
      ],
    });
  });

  it.each([
    ["negative", "-1"],
    ["positive infinity", "1e309"],
    ["negative infinity", "-1e309"],
    ["NaN", "NaN"],
  ])("rejects %s cooldowns", (_name, seconds) => {
    expect(() =>
      extract(`
        flaggo.tune.number("tetris.dropInterval", {
          definition: ${cooldownDefinition(seconds)},
          context: {}
        });
      `)
    ).toThrowError(StaticExtractionError);
  });

  it("rejects invalid numeric value contracts and descending ranges", () => {
    const invalidDefinitions = [
      numberDefinition().replace("min: 200", "min: 1600"),
      numberDefinition().replace("default: 800", "default: 825"),
      numberDefinition().replace("value: 800", "value: 825"),
    ];
    for (const definition of invalidDefinitions) {
      expect(() =>
        extract(`
          flaggo.tune.number("tetris.dropInterval", {
            definition: ${definition},
            context: {}
          });
        `)
      ).toThrowError(
        expect.objectContaining<Partial<StaticExtractionError>>({
          code: "invalid-static-definition",
        }),
      );
    }

    expect(() =>
      extract(`
        createInferenceSignalHandle({
          kind: "metric",
          key: "tetris.boardPressure",
          type: "number",
          source: "app-emitted",
          range: [1, 0]
        }, sink);
        flaggo.tune.number("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `)
    ).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "invalid-static-definition",
      }),
    );
  });

  it("resolves directly imported signal handles in static semantics", () => {
    const rootDir = mkdtempSync(join(tmpdir(), "flaggo-extract-"));
    temporaryDirectories.push(rootDir);
    const signalsFile = join(rootDir, "signals.ts");
    const appFile = join(rootDir, "app.ts");
    writeFileSync(
      signalsFile,
      `
        import { createInferenceSignalHandle } from "@flaggo/sdk";
        export const boardPressureSignal = createInferenceSignalHandle({
          kind: "metric",
          key: "tetris.boardPressure",
          type: "number",
          source: "app-emitted",
          range: [0, 1]
        }, sink);
      `,
      "utf8",
    );
    writeFileSync(
      appFile,
      `
        import { boardPressureSignal } from "./signals.js";
        flaggo.tune.number("tetris.dropInterval", {
          definition: {
            key: "tetris.dropInterval",
            valueType: "number",
            actionSpace: {
              type: "number",
              min: 200,
              max: 1500,
              step: 50,
              default: 800
            },
            fallback: { value: 800 },
            signals: { evidence: [boardPressureSignal] },
            inference: {
              target: "session",
              inputs: [boardPressureSignal]
            },
            intent: {
              type: "metric-objective",
              primary: {
                signal: boardPressureSignal,
                direction: "minimize"
              }
            },
            policy: { kind: "inline", constraints: [] }
          },
          inputs: [boardPressureSignal.input(boardPressure)],
          context: {}
        });
      `,
      "utf8",
    );

    const result = extractDecisionBundle({
      fileNames: [appFile],
      rootDir,
      application: { id: "tetris-demo", environment: "dev" },
      source: { repository: "flaggo" },
    });

    expect(result.bundle.signals).toEqual([
      expect.objectContaining({ key: "tetris.boardPressure" }),
    ]);
    expect(result.bundle.definitions[0]).toMatchObject({
      signals: {
        allowed: [{ key: "tetris.boardPressure" }],
        evidence: [{ key: "tetris.boardPressure" }],
      },
      inference: {
        inputs: [{ key: "tetris.boardPressure" }],
      },
      intent: {
        primary: { signal: { key: "tetris.boardPressure" } },
      },
    });
  }, 15_000);

  it("rejects every invalid supplied signal digest before deduplication", () => {
    expect(() =>
      extract(`
        createInferenceSignalHandle({
          kind: "metric",
          key: "tetris.boardPressure",
          type: "number",
          source: "app-emitted",
          schemaDigest: "sha256:${"0".repeat(64)}"
        }, sink);
        createInferenceSignalHandle({
          kind: "metric",
          key: "tetris.boardPressure",
          type: "number",
          source: "app-emitted"
        }, sink);
        flaggo.tune.number("tetris.dropInterval", {
          definition: ${numberDefinition()},
          context: {}
        });
      `)
    ).toThrowError(
      expect.objectContaining<Partial<StaticExtractionError>>({
        code: "invalid-static-definition",
      }),
    );
  });

  it("writes the generated artifact through the extraction CLI", async () => {
    const definition = numberDefinition();
    const { fileName, rootDir } = sourceFile(`
      flaggo.tune.number("tetris.dropInterval", {
        definition: ${definition},
        context: {}
      });
    `);
    const project = join(rootDir, "tsconfig.json");
    const output = join(rootDir, ".flaggo", "definitions.json");
    writeFileSync(
      project,
      JSON.stringify({
        compilerOptions: {
          target: "ES2022",
          module: "NodeNext",
          moduleResolution: "NodeNext",
          noResolve: true,
        },
        files: [fileName],
      }),
      "utf8",
    );

    await runExtractorCli([
      "--project",
      project,
      "--app",
      "tetris-demo",
      "--environment",
      "dev",
      "--out",
      output,
      "--repository",
      "flaggo",
      "--source-path",
      "apps/tetris",
    ]);

    const artifact = JSON.parse(readFileSync(output, "utf8")) as {
      format: string;
      bundle: { definitions: unknown[] };
    };
    expect(artifact.format).toBe("flaggo.static-extraction/v1");
    expect(artifact.bundle.definitions).toHaveLength(1);
  });
});
