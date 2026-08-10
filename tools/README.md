# Development Tools

Repository tooling provides stable entrypoints for validation and local
execution.

`python tools/dev.py check` runs the baseline contract gate.
`python tools/dev.py serve` starts the fixture-backed API. Tooling must remain
cloud-independent and surface failures directly.

`Flaggo.DirectoryFlush` is the checked-in Windows helper used by the Node
bootstrap for handle-verified no-follow directory creation, immutable writes,
atomic descriptor/generation publication, committed generation reads, and
directory flushing. Native rename completion is the publication commit point;
post-rename path validation or directory-flush failures are surfaced without
deleting the now-current artifact or generation. Root/component handles stay
locally owned until captured and are closed on every constructor failure. The
helper is built as part of `Flaggo.slnx`.
