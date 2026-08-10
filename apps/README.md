# Applications

`apps` contains independently runnable Flaggo hosts. Applications compose
module-owned interfaces with infrastructure adapters; domain modules never
depend on application projects.

Initial hosts are the control plane, data plane, and operator console. A future
worker may be added when asynchronous intelligence requires an independent
process.

`apps/shared/src/Flaggo.Hosting` contains strict HTTP contract mechanics shared
by executable hosts without owning routes or business behavior. Management
routes exist only in `Flaggo.ControlPlane`; decide, exposure, and health routes
exist only in `Flaggo.DataPlane`.

For local development, both hosts compose the registry-owned
`LocalFileDefinitionRegistry` through the shared hosting configuration. With no
override they resolve the same repository-local
`.flaggo/definition-registry-v1.json`. Set the absolute
`Flaggo__Registry__LocalFilePath` environment variable when hosts use different
working directories or when tests/deployments require isolated state. The
directory is created on first access; invalid or inaccessible state is an
explicit startup/runtime failure, never a silent in-memory fallback.

Data-plane direct state/evidence overrides use
`Flaggo__State__LocalFilePath` and `Flaggo__Evidence__LocalFilePath` as commit
descriptor paths. They never consume mutable raw JSON. Deployments that switch
receipt, state, and evidence together should configure
`Flaggo__Bootstrap__LocalGenerationPath` and publish the single digest-pinned
generation manifest last.
