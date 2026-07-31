# Development Tools

Repository tooling provides stable entrypoints for validation and local
execution.

`python tools/dev.py check` runs the baseline contract gate.
`python tools/dev.py serve` starts the fixture-backed API. Tooling must remain
cloud-independent and surface failures directly.
