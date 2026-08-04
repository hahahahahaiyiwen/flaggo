import { isAbsolute, relative, resolve } from "node:path";
import * as ts from "typescript";

import {
  bundleDigest,
  compareCanonicalStrings,
  contractDigest,
  normalizeBundle,
  signalSchemaDigest,
} from "./canonical.js";
import {
  isFrozenNumberDecisionDefinition,
  isFrozenSignalDeclaration,
} from "./static-schema.js";
import type {
  DecisionDefinitionBundle,
  NumberDecisionDefinition,
  Sha256Digest,
  SignalDeclaration,
} from "./types.js";

export type StaticExtractionErrorCode =
  | "contract-conflict"
  | "invalid-static-definition"
  | "source-outside-root"
  | "static-key-required"
  | "unsupported-static-syntax";

export class StaticExtractionError extends Error {
  override readonly name = "StaticExtractionError";

  constructor(
    readonly code: StaticExtractionErrorCode,
    message: string,
    readonly sourceFile?: string,
    readonly line?: number,
    readonly column?: number,
  ) {
    super(
      sourceFile === undefined
        ? `[${code}] ${message}`
        : `[${code}] ${sourceFile}:${line}:${column}: ${message}`,
    );
  }
}

export interface StaticDecisionDescriptor {
  decisionKey: string;
  contractDigest: Sha256Digest;
  sourceFile: string;
  line: number;
  column: number;
}

export interface StaticExtractionResult {
  format: "flaggo.static-extraction/v1";
  bundle: DecisionDefinitionBundle;
  bundleDigest: Sha256Digest;
  descriptors: StaticDecisionDescriptor[];
}

export interface StaticExtractionOptions {
  fileNames: string[];
  rootDir: string;
  application: {
    id: string;
    environment: string;
  };
  source: DecisionDefinitionBundle["source"];
  build?: DecisionDefinitionBundle["build"];
  compilerOptions?: ts.CompilerOptions;
}

type JsonValue =
  | boolean
  | number
  | string
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

interface SourceContext {
  rootDir: string;
  source: ts.SourceFile;
}

type StaticIdentifierResolver = (
  identifier: ts.Identifier,
) => JsonValue | undefined;

function unwrap(node: ts.Expression): ts.Expression {
  let current = node;
  while (
    ts.isParenthesizedExpression(current)
    || ts.isAsExpression(current)
    || ts.isTypeAssertionExpression(current)
    || ts.isSatisfiesExpression(current)
    || ts.isNonNullExpression(current)
  ) {
    current = current.expression;
  }
  return current;
}

function location(
  node: ts.Node,
  context: SourceContext,
): Pick<StaticDecisionDescriptor, "sourceFile" | "line" | "column"> {
  const sourceFile = relative(
    context.rootDir,
    context.source.fileName,
  ).replaceAll("\\", "/");
  if (
    sourceFile === ".."
    || sourceFile.startsWith("../")
    || isAbsolute(sourceFile)
  ) {
    throw extractionError(
      "source-outside-root",
      "Extracted source must be contained by rootDir.",
      node,
      context,
    );
  }
  const position = context.source.getLineAndCharacterOfPosition(
    node.getStart(context.source),
  );
  return {
    sourceFile,
    line: position.line + 1,
    column: position.character + 1,
  };
}

function extractionError(
  code: StaticExtractionErrorCode,
  message: string,
  node: ts.Node,
  context: SourceContext,
): StaticExtractionError {
  const sourceFile = relative(
    context.rootDir,
    context.source.fileName,
  ).replaceAll("\\", "/");
  const position = context.source.getLineAndCharacterOfPosition(
    node.getStart(context.source),
  );
  return new StaticExtractionError(
    code,
    message,
    sourceFile,
    position.line + 1,
    position.character + 1,
  );
}

function propertyName(
  name: ts.PropertyName,
  node: ts.Node,
  context: SourceContext,
): string {
  if (ts.isIdentifier(name) || ts.isStringLiteral(name)) return name.text;
  if (ts.isNumericLiteral(name)) return name.text;
  throw extractionError(
    "unsupported-static-syntax",
    "Computed property names are not extractable.",
    node,
    context,
  );
}

function staticValue(
  node: ts.Expression,
  context: SourceContext,
  resolveIdentifier?: StaticIdentifierResolver,
): JsonValue {
  const expression = unwrap(node);
  if (ts.isIdentifier(expression) && resolveIdentifier !== undefined) {
    const resolved = resolveIdentifier(expression);
    if (resolved !== undefined) return structuredClone(resolved);
  }
  if (
    ts.isStringLiteral(expression)
    || ts.isNoSubstitutionTemplateLiteral(expression)
  ) {
    return expression.text;
  }
  if (ts.isNumericLiteral(expression)) return Number(expression.text);
  if (expression.kind === ts.SyntaxKind.TrueKeyword) return true;
  if (expression.kind === ts.SyntaxKind.FalseKeyword) return false;
  if (expression.kind === ts.SyntaxKind.NullKeyword) return null;
  if (
    ts.isPrefixUnaryExpression(expression)
    && (
      expression.operator === ts.SyntaxKind.MinusToken
      || expression.operator === ts.SyntaxKind.PlusToken
    )
    && ts.isNumericLiteral(expression.operand)
  ) {
    const value = Number(expression.operand.text);
    return expression.operator === ts.SyntaxKind.MinusToken ? -value : value;
  }
  if (ts.isArrayLiteralExpression(expression)) {
    return expression.elements.map((element) => {
      if (ts.isSpreadElement(element) || ts.isOmittedExpression(element)) {
        throw extractionError(
          "unsupported-static-syntax",
          "Array spreads and omitted elements are not extractable.",
          element,
          context,
        );
      }
      return staticValue(element, context, resolveIdentifier);
    });
  }
  if (ts.isObjectLiteralExpression(expression)) {
    const result: Record<string, JsonValue> = {};
    for (const property of expression.properties) {
      if (!ts.isPropertyAssignment(property)) {
        throw extractionError(
          "unsupported-static-syntax",
          "Static objects require direct property assignments without spreads, shorthand, or methods.",
          property,
          context,
        );
      }
      const key = propertyName(property.name, property, context);
      if (Object.hasOwn(result, key)) {
        throw extractionError(
          "unsupported-static-syntax",
          `Duplicate static property '${key}' is not allowed.`,
          property,
          context,
        );
      }
      result[key] = staticValue(
        property.initializer,
        context,
        resolveIdentifier,
      );
    }
    return result;
  }
  throw extractionError(
    "unsupported-static-syntax",
    "Static definitions may contain only JSON literals, literal arrays, and literal objects.",
    expression,
    context,
  );
}

function directProperty(
  object: ts.ObjectLiteralExpression,
  key: string,
  context: SourceContext,
): ts.Expression | undefined {
  for (const property of object.properties) {
    if (!ts.isPropertyAssignment(property)) {
      if (ts.isSpreadAssignment(property)) {
        throw extractionError(
          "unsupported-static-syntax",
          "Request objects containing spreads are not extractable.",
          property,
          context,
        );
      }
      continue;
    }
    if (propertyName(property.name, property, context) === key) {
      return property.initializer;
    }
  }
  return undefined;
}

function isFlaggoRoot(expression: ts.Expression): boolean {
  const value = unwrap(expression);
  return ts.isIdentifier(value) && value.text === "flaggo";
}

function isDirectFlaggoTune(expression: ts.Expression): boolean {
  const value = unwrap(expression);
  if (ts.isPropertyAccessExpression(value)) {
    return value.name.text === "tune" && isFlaggoRoot(value.expression);
  }
  return ts.isElementAccessExpression(value)
    && isFlaggoRoot(value.expression);
}

function tuneMethod(
  call: ts.CallExpression,
): "supported" | "unsupported" | undefined {
  const expression = unwrap(call.expression);
  if (ts.isElementAccessExpression(expression)) {
    const receiver = unwrap(expression.expression);
    if (isDirectFlaggoTune(receiver)) return "unsupported";
    return undefined;
  }
  if (!ts.isPropertyAccessExpression(expression)) return undefined;
  if (
    expression.name.text !== "number"
    && expression.name.text !== "numberDetailed"
  ) {
    return undefined;
  }
  const receiver = unwrap(expression.expression);
  if (ts.isElementAccessExpression(receiver)) {
    return isFlaggoRoot(receiver.expression) ? "unsupported" : undefined;
  }
  if (
    !ts.isPropertyAccessExpression(receiver)
    || !isDirectFlaggoTune(receiver)
  ) {
    return undefined;
  }
  return call.questionDotToken !== undefined
      || expression.questionDotToken !== undefined
      || receiver.questionDotToken !== undefined
    ? "unsupported"
    : "supported";
}

function signalFactoryName(expression: ts.Expression): string | undefined {
  const value = unwrap(expression);
  const name = ts.isIdentifier(value)
    ? value.text
    : ts.isPropertyAccessExpression(value)
      ? value.name.text
      : undefined;
  return name === "createSignalHandle"
    || name === "createInferenceSignalHandle"
    || name === "createDerivedMetricHandle"
    ? name
    : undefined;
}

function conditionalContainer(node: ts.Node): boolean {
  if (
    ts.isIfStatement(node)
    || ts.isConditionalExpression(node)
    || ts.isForStatement(node)
    || ts.isForInStatement(node)
    || ts.isForOfStatement(node)
    || ts.isWhileStatement(node)
    || ts.isDoStatement(node)
    || ts.isSwitchStatement(node)
  ) {
    return true;
  }
  return ts.isBinaryExpression(node)
    && (
      node.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken
      || node.operatorToken.kind === ts.SyntaxKind.BarBarToken
      || node.operatorToken.kind === ts.SyntaxKind.QuestionQuestionToken
    );
}

export function extractDecisionBundle(
  options: StaticExtractionOptions,
): StaticExtractionResult {
  if (
    options.application.id.length === 0
    || options.application.environment.length === 0
  ) {
    throw new StaticExtractionError(
      "invalid-static-definition",
      "Application id and environment must be non-empty.",
    );
  }
  const rootDir = resolve(options.rootDir);
  const program = ts.createProgram(
    options.fileNames.map((fileName) => resolve(fileName)),
    {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.NodeNext,
      moduleResolution: ts.ModuleResolutionKind.NodeNext,
      ...options.compilerOptions,
      noLib: true,
      skipLibCheck: true,
    },
  );
  const definitions: NumberDecisionDefinition[] = [];
  const rawDescriptors: Array<
    Omit<StaticDecisionDescriptor, "contractDigest"> & {
      definition: NumberDecisionDefinition;
    }
  > = [];
  const signals = new Map<string, SignalDeclaration>();
  const contexts = new WeakMap<ts.Node, SourceContext>();
  const sourceFiles = program.getSourceFiles().filter((sourceFile) => {
    if (sourceFile.isDeclarationFile) return false;
    const sourcePath = relative(rootDir, resolve(sourceFile.fileName))
      .replaceAll("\\", "/");
    return sourcePath !== ".."
      && !sourcePath.startsWith("../")
      && !isAbsolute(sourcePath)
      && !sourcePath.split("/").includes("node_modules");
  });
  for (const sourceFile of sourceFiles) {
    const context = { rootDir, source: sourceFile };
    const mapContext = (node: ts.Node): void => {
      contexts.set(node, context);
      ts.forEachChild(node, mapContext);
    };
    mapContext(sourceFile);
  }

  let checker: ts.TypeChecker | undefined;
  const resolvedSignals = new Map<ts.Symbol, SignalDeclaration>();
  const resolvingSignals = new Set<ts.Symbol>();
  const rootClientAliases = new Set<ts.Symbol>();
  const tuneObjectAliases = new Set<ts.Symbol>();
  const tuneMethodAliases = new Set<ts.Symbol>();
  const signalFactoryImports = new WeakMap<
    ts.SourceFile,
    { named: Set<ts.Symbol>; namespaces: Set<ts.Symbol> }
  >();

  const localSymbol = (identifier: ts.Identifier): ts.Symbol | undefined => {
    checker ??= program.getTypeChecker();
    return checker.getSymbolAtLocation(identifier);
  };

  const resolveSymbol = (identifier: ts.Identifier): ts.Symbol | undefined => {
    let symbol = localSymbol(identifier);
    if (symbol !== undefined && (symbol.flags & ts.SymbolFlags.Alias) !== 0) {
      checker ??= program.getTypeChecker();
      symbol = checker.getAliasedSymbol(symbol);
    }
    return symbol;
  };

  const addAliasSymbol = (
    aliases: Set<ts.Symbol>,
    identifier: ts.Identifier,
  ): boolean => {
    let changed = false;
    for (const symbol of [localSymbol(identifier), resolveSymbol(identifier)]) {
      if (symbol !== undefined && !aliases.has(symbol)) {
        aliases.add(symbol);
        changed = true;
      }
    }
    return changed;
  };

  const hasAliasSymbol = (
    aliases: Set<ts.Symbol>,
    identifier: ts.Identifier,
  ): boolean => {
    const local = localSymbol(identifier);
    const resolved = resolveSymbol(identifier);
    return (local !== undefined && aliases.has(local))
      || (resolved !== undefined && aliases.has(resolved));
  };

  const isFlaggoRootReference = (expression: ts.Expression): boolean => {
    const value = unwrap(expression);
    return isFlaggoRoot(value)
      || (ts.isIdentifier(value) && hasAliasSymbol(rootClientAliases, value));
  };

  const isFlaggoTuneReference = (expression: ts.Expression): boolean => {
    const value = unwrap(expression);
    if (ts.isPropertyAccessExpression(value)) {
      return value.name.text === "tune"
        && isFlaggoRootReference(value.expression);
    }
    return ts.isElementAccessExpression(value)
      && isFlaggoRootReference(value.expression);
  };

  const bindingName = (element: ts.BindingElement): string | undefined => {
    const name = element.propertyName ?? element.name;
    return ts.isIdentifier(name) || ts.isStringLiteral(name)
      ? name.text
      : undefined;
  };

  const addBindingAliases = (
    pattern: ts.ObjectBindingPattern,
    source: "flaggo" | "tune",
  ): boolean => {
    let changed = false;
    for (const element of pattern.elements) {
      const property = bindingName(element);
      if (source === "flaggo" && property === "tune") {
        if (ts.isIdentifier(element.name)) {
          changed =
            addAliasSymbol(tuneObjectAliases, element.name) || changed;
        } else if (ts.isObjectBindingPattern(element.name)) {
          changed = addBindingAliases(element.name, "tune") || changed;
        }
      }
      if (
        source === "tune"
        && (property === "number" || property === "numberDetailed")
        && ts.isIdentifier(element.name)
      ) {
        changed =
          addAliasSymbol(tuneMethodAliases, element.name) || changed;
      }
    }
    return changed;
  };

  const bindingDeclarations: ts.VariableDeclaration[] = [];
  const rootAliasDeclarations: ts.VariableDeclaration[] = [];
  for (const sourceFile of sourceFiles) {
    const collectBindings = (node: ts.Node): void => {
      if (
        ts.isVariableDeclaration(node)
        && node.initializer !== undefined
      ) {
        if (ts.isObjectBindingPattern(node.name)) {
          bindingDeclarations.push(node);
        } else if (ts.isIdentifier(node.name)) {
          rootAliasDeclarations.push(node);
        }
      }
      ts.forEachChild(node, collectBindings);
    };
    collectBindings(sourceFile);
    for (const statement of sourceFile.statements) {
      if (
        !ts.isImportDeclaration(statement)
        || !ts.isStringLiteral(statement.moduleSpecifier)
      ) {
        continue;
      }
      const bindings = statement.importClause?.namedBindings;
      if (bindings !== undefined && ts.isNamedImports(bindings)) {
        for (const element of bindings.elements) {
          if ((element.propertyName ?? element.name).text === "flaggo") {
            addAliasSymbol(rootClientAliases, element.name);
          }
        }
      }
      if (statement.moduleSpecifier.text === "@flaggo/sdk") {
        const imports = signalFactoryImports.get(sourceFile)
          ?? {
            named: new Set<ts.Symbol>(),
            namespaces: new Set<ts.Symbol>(),
          };
        if (bindings !== undefined && ts.isNamedImports(bindings)) {
          for (const element of bindings.elements) {
            if (
              signalFactoryName(element.propertyName ?? element.name)
              !== undefined
            ) {
              const symbol = localSymbol(element.name);
              if (symbol !== undefined) imports.named.add(symbol);
            }
          }
        } else if (bindings !== undefined && ts.isNamespaceImport(bindings)) {
          const symbol = localSymbol(bindings.name);
          if (symbol !== undefined) imports.namespaces.add(symbol);
        }
        signalFactoryImports.set(sourceFile, imports);
      }
    }
  }

  const isFlaggoSignalFactory = (
    call: ts.CallExpression,
    context: SourceContext,
  ): boolean => {
    const imports = signalFactoryImports.get(context.source);
    if (imports === undefined) return false;
    const expression = unwrap(call.expression);
    if (ts.isIdentifier(expression)) {
      const symbol = localSymbol(expression);
      return symbol !== undefined && imports.named.has(symbol);
    }
    if (!ts.isPropertyAccessExpression(expression)) return false;
    const receiver = unwrap(expression.expression);
    const symbol = ts.isIdentifier(receiver)
      ? localSymbol(receiver)
      : undefined;
    return symbol !== undefined
      && imports.namespaces.has(symbol)
      && signalFactoryName(expression) !== undefined;
  };
  let aliasesChanged: boolean;
  do {
    aliasesChanged = false;
    for (const declaration of rootAliasDeclarations) {
      if (
        ts.isIdentifier(declaration.name)
        && declaration.initializer !== undefined
        && isFlaggoRootReference(declaration.initializer)
      ) {
        aliasesChanged =
          addAliasSymbol(rootClientAliases, declaration.name)
          || aliasesChanged;
      }
    }
    for (const declaration of bindingDeclarations) {
      const initializer = unwrap(declaration.initializer!);
      let source: "flaggo" | "tune" | undefined;
      if (isFlaggoRootReference(initializer)) {
        source = "flaggo";
      } else if (isFlaggoTuneReference(initializer)) {
        source = "tune";
      } else if (ts.isIdentifier(initializer)) {
        const symbol = resolveSymbol(initializer);
        if (symbol !== undefined && tuneObjectAliases.has(symbol)) {
          source = "tune";
        }
      }
      if (source !== undefined && ts.isObjectBindingPattern(declaration.name)) {
        aliasesChanged =
          addBindingAliases(declaration.name, source) || aliasesChanged;
      }
    }
  } while (aliasesChanged);

  const variableInitializer = (
    identifier: ts.Identifier,
    resolving: Set<ts.Symbol>,
  ): ts.Expression | undefined => {
    const symbol = resolveSymbol(identifier);
    if (symbol === undefined || resolving.has(symbol)) return undefined;
    const declaration = symbol.valueDeclaration
      ?? symbol.declarations?.find(ts.isVariableDeclaration);
    if (
      declaration === undefined
      || !ts.isVariableDeclaration(declaration)
      || declaration.initializer === undefined
    ) {
      return undefined;
    }
    resolving.add(symbol);
    return declaration.initializer;
  };

  const isTuneObjectReference = (
    expression: ts.Expression,
    resolving: Set<ts.Symbol>,
  ): boolean => {
    const value = unwrap(expression);
    if (isFlaggoTuneReference(value)) return true;
    if (!ts.isIdentifier(value)) return false;
    if (hasAliasSymbol(tuneObjectAliases, value)) return true;
    const initializer = variableInitializer(value, resolving);
    return initializer !== undefined
      && isTuneObjectReference(initializer, resolving);
  };

  const isTuneMethodReference = (
    expression: ts.Expression,
    resolving = new Set<ts.Symbol>(),
  ): boolean => {
    const value = unwrap(expression);
    if (ts.isIdentifier(value)) {
      if (hasAliasSymbol(tuneMethodAliases, value)) return true;
      const initializer = variableInitializer(value, resolving);
      return initializer !== undefined
        && isTuneMethodReference(initializer, resolving);
    }
    if (
      ts.isPropertyAccessExpression(value)
      && (
        value.name.text === "number"
        || value.name.text === "numberDetailed"
      )
    ) {
      return isTuneObjectReference(value.expression, resolving);
    }
    return ts.isElementAccessExpression(value)
      && isTuneObjectReference(value.expression, resolving);
  };

  const addSignal = (
    value: SignalDeclaration,
    node: ts.Node,
    context: SourceContext,
  ): void => {
    const computedDigest = signalSchemaDigest(value);
    if (
      value.schemaDigest !== undefined
      && value.schemaDigest !== computedDigest
    ) {
      throw extractionError(
        "invalid-static-definition",
        `Schema digest mismatch for signal declaration '${value.key}'.`,
        node,
        context,
      );
    }
    const previous = signals.get(value.key);
    if (
      previous !== undefined
      && signalSchemaDigest(previous) !== computedDigest
    ) {
      throw extractionError(
        "contract-conflict",
        `Conflicting signal declaration '${value.key}'.`,
        node,
        context,
      );
    }
    if (previous !== undefined) return;
    signals.set(value.key, value);
  };

  const resolveSignalIdentifier: StaticIdentifierResolver = (identifier) => {
    const symbol = resolveSymbol(identifier);
    if (symbol === undefined) return undefined;
    const cached = resolvedSignals.get(symbol);
    if (cached !== undefined) return { key: cached.key };
    if (resolvingSignals.has(symbol)) {
      const context = contexts.get(identifier);
      if (context === undefined) return undefined;
      throw extractionError(
        "unsupported-static-syntax",
        `Circular signal declaration '${identifier.text}' is not extractable.`,
        identifier,
        context,
      );
    }
    const declaration = symbol.valueDeclaration
      ?? symbol.declarations?.find(ts.isVariableDeclaration);
    if (
      declaration === undefined
      || !ts.isVariableDeclaration(declaration)
      || declaration.initializer === undefined
    ) {
      return undefined;
    }
    const initializer = unwrap(declaration.initializer);
    const context = contexts.get(declaration);
    if (
      !ts.isCallExpression(initializer)
      || context === undefined
      || !isFlaggoSignalFactory(initializer, context)
    ) {
      return undefined;
    }
    const declarationNode = initializer.arguments[0];
    if (declarationNode === undefined || context === undefined) return undefined;

    resolvingSignals.add(symbol);
    let value: JsonValue;
    try {
      value = staticValue(
        declarationNode,
        context,
        resolveSignalIdentifier,
      );
    } finally {
      resolvingSignals.delete(symbol);
    }
    if (!isFrozenSignalDeclaration(value)) {
      throw extractionError(
        "invalid-static-definition",
        "The extracted signal declaration is invalid.",
        declarationNode,
        context,
      );
    }
    resolvedSignals.set(symbol, value);
    addSignal(value, declarationNode, context);
    return { key: value.key };
  };

  for (const sourceFile of sourceFiles) {
    const context = { rootDir, source: sourceFile };
    const syntaxError = program.getSyntacticDiagnostics(sourceFile)[0];
    if (syntaxError !== undefined) {
      const node = syntaxError.start === undefined
        ? sourceFile
        : findNodeAt(sourceFile, syntaxError.start);
      throw extractionError(
        "unsupported-static-syntax",
        ts.flattenDiagnosticMessageText(syntaxError.messageText, "\n"),
        node,
        context,
      );
    }

    const visit = (node: ts.Node, conditional: boolean): void => {
      const tuneInvocation = ts.isCallExpression(node)
        ? tuneMethod(node) ?? (
            isTuneMethodReference(node.expression)
              ? "unsupported"
              : undefined
          )
        : undefined;
      if (tuneInvocation === "unsupported") {
        throw extractionError(
          "unsupported-static-syntax",
          "Computed tune method access is not extractable.",
          node,
          context,
        );
      }
      if (ts.isCallExpression(node) && tuneInvocation === "supported") {
        if (conditional) {
          throw extractionError(
            "unsupported-static-syntax",
            "Conditional or loop-dependent decision calls are not extractable.",
            node,
            context,
          );
        }
        const keyArgument = node.arguments[0];
        const requestArgument = node.arguments[1];
        if (
          keyArgument === undefined
          || !ts.isStringLiteral(unwrap(keyArgument))
        ) {
          throw extractionError(
            "static-key-required",
            "Decision keys must be direct string literals.",
            keyArgument ?? node,
            context,
          );
        }
        const request = requestArgument === undefined
          ? undefined
          : unwrap(requestArgument);
        if (request === undefined || !ts.isObjectLiteralExpression(request)) {
          throw extractionError(
            "unsupported-static-syntax",
            "Decision requests must be direct object literals.",
            requestArgument ?? node,
            context,
          );
        }
        const definitionNode = directProperty(request, "definition", context);
        if (definitionNode === undefined) {
          throw extractionError(
            "invalid-static-definition",
            "An extractable decision call must contain a definition property.",
            request,
            context,
          );
        }
        const value = staticValue(
          definitionNode,
          context,
          resolveSignalIdentifier,
        );
        if (!isFrozenNumberDecisionDefinition(value)) {
          throw extractionError(
            "invalid-static-definition",
            "The extracted definition is not a valid numeric decision definition.",
            definitionNode,
            context,
          );
        }
        const decisionKey = (unwrap(keyArgument) as ts.StringLiteral).text;
        if (value.key !== decisionKey) {
          throw extractionError(
            "invalid-static-definition",
            `Definition key '${value.key}' does not match call-site key '${decisionKey}'.`,
            definitionNode,
            context,
          );
        }
        definitions.push(value);
        rawDescriptors.push({
          decisionKey,
          definition: value,
          ...location(node, context),
        });
      } else if (
        ts.isCallExpression(node)
        && isFlaggoSignalFactory(node, context)
      ) {
        const declarationNode = node.arguments[0];
        if (declarationNode === undefined) {
          throw extractionError(
            "invalid-static-definition",
            "Signal factories require a literal declaration.",
            node,
            context,
          );
        }
        const value = staticValue(
          declarationNode,
          context,
          resolveSignalIdentifier,
        );
        if (!isFrozenSignalDeclaration(value)) {
          throw extractionError(
            "invalid-static-definition",
            "The extracted signal declaration is invalid.",
            declarationNode,
            context,
          );
        }
        addSignal(value, declarationNode, context);
      }
      const childConditional = conditional || conditionalContainer(node);
      ts.forEachChild(node, (child) => visit(child, childConditional));
    };
    visit(sourceFile, false);
  }

  if (definitions.length === 0) {
    throw new StaticExtractionError(
      "invalid-static-definition",
      "No extractable numeric decision definitions were found.",
    );
  }

  let bundle: DecisionDefinitionBundle;
  try {
    bundle = normalizeBundle({
      format: "flaggo.decision-definition-bundle/v1",
      application: structuredClone(options.application),
      ...(options.build === undefined
        ? {}
        : { build: structuredClone(options.build) }),
      source: structuredClone(options.source),
      ...(signals.size === 0 ? {} : { signals: [...signals.values()] }),
      definitions,
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new StaticExtractionError(
      message.includes("conflicting decision definition key")
        || message.includes("duplicate decision key")
        ? "contract-conflict"
        : "invalid-static-definition",
      message,
    );
  }

  const normalizedDefinitions = new Map(
    bundle.definitions.map((definition) => [definition.key, definition]),
  );
  const descriptors = rawDescriptors
    .map(({ definition: _definition, ...descriptor }) => {
      const definition = normalizedDefinitions.get(descriptor.decisionKey);
      if (definition === undefined) {
        throw new StaticExtractionError(
          "invalid-static-definition",
          `No normalized definition exists for '${descriptor.decisionKey}'.`,
        );
      }
      return {
        ...descriptor,
        contractDigest: contractDigest(definition),
      };
    })
    .sort((left, right) =>
      compareCanonicalStrings(left.sourceFile, right.sourceFile)
      || left.line - right.line
      || left.column - right.column
      || compareCanonicalStrings(left.decisionKey, right.decisionKey)
    );

  return {
    format: "flaggo.static-extraction/v1",
    bundle,
    bundleDigest: bundleDigest(bundle),
    descriptors,
  };
}

function findNodeAt(node: ts.Node, position: number): ts.Node {
  let found = node;
  node.forEachChild((child) => {
    if (child.getFullStart() <= position && child.getEnd() >= position) {
      found = findNodeAt(child, position);
    }
  });
  return found;
}
