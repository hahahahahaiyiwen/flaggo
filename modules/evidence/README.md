# Evidence Module

Owns signal ingestion boundaries, evidence snapshots, aggregation lineage, and
quality information consumed by runtime and asynchronous intelligence.

It returns domain evidence types through async ports and does not select
actions. Missing evidence remains distinguishable from low-quality evidence,
and detailed evidence stays in audit rather than compact runtime responses.

Update this document when signal ownership, aggregation, or quality semantics
change.
