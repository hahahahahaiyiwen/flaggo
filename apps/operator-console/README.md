# Operator Console

The operator console is the human governance interface for inspection,
approval, override, pause/resume, and rollback.

It is a control-plane client and must not bypass management API authorization
or mutate module storage directly. UI-specific models adapt from published
contracts rather than becoming shared domain contracts.
