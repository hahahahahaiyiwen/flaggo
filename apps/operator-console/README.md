# Operator Console

The operator console is a future client application, not a first-class server
component. It may provide human governance UX for inspection, approval,
override, pause/resume, and rollback.

It calls Contract Service, Decision Service query surfaces, and store-backed
read APIs. It must not bypass authorization or mutate storage directly.
UI-specific models adapt from published contracts rather than becoming shared
domain contracts.
