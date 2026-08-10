# Windows Durable Publication Helper

This Windows-only executable is the native durability boundary for the Node
Tetris bootstrap. It traverses components relative to pinned handles with
`NtCreateFile`, rejects reparse points, and validates exact final paths plus
volume/file identities for every opened handle. It safely creates and flushes
directory trees, writes immutable files, publishes descriptors and generation
pointers with handle-relative atomic rename, and resolves committed
generations by reading the exact validated handles.

`durable-json.mjs` starts the built helper in JSON-lines server mode so a
publication can reuse one process across ordered durability barriers. Requests
and results are JSON lines with stable error codes. Staging handles are marked
for deletion on pre-publication failures. Native rename is the commit point and
is recorded before any post-rename bookkeeping, validation, or directory
flush, so later failures remain surfaced without deleting committed state.
Concurrent Node callers share one startup promise; startup failure, idle
timeout, process exit/signals, and test reset all reap the one owned child and
permit retry where applicable. Build `Flaggo.slnx` before running the bootstrap
on Windows. Do not weaken failures to best-effort success or add unmanaged Node
binary dependencies.

The `FLAGGO_DIRECTORY_HELPER_TEST_HOOKS=1` gate exists only for deterministic
Windows swap-back race tests. Production callers never enable or send hooks.
