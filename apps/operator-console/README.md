# Operator Console

**Status:** planned client; no runnable application is implemented in this
directory.

Operator Console may eventually provide human governance UX for inspection,
activation, override, pause/resume, and rollback. It is not a server authority
and is not shown as a server component in the
[canonical lifecycle](../../docs/design/architecture/APP_BOUNDARIES.md).

The client must use authenticated Contract Service or future explicit
operational APIs. It must not access Contract Store, Executable Store, Evidence
Store, or activation records directly. UI-specific models adapt from public
contracts rather than becoming shared domain contracts.
