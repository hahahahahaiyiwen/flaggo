# Tetris integration example

This example exercises the complete v3 path:

```text
DecisionContract
  -> Contract Service publication
  -> authored executable auto-activation
  -> exact-version SDK request
  -> Decision Service RuntimeDecision
```

`decision-contract.json` defines the runtime attributes, numeric result schema,
default, authored rules, and learning declarations. The integration starts
Contract Service and Decision Service against one isolated SQLite database,
validates and publishes the contract, and passes the returned
`{ contractName, contractDigest }` binding to the TypeScript SDK.

The checks cover high- and low-pressure authored rules, default evaluation
when rule attributes are missing, SDK-owned `_random`, exact digest
provenance, absence of retired target/idempotency/fallback fields, and explicit
failure when Decision Service is unavailable.

Run from the repository root:

```powershell
npm run test:tetris-integration
```
