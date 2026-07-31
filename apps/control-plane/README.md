# Control Plane

The control-plane host exposes definition validation, bundle application, and
approval lifecycle operations.

It composes registry, policy, state, and audit ports. It never serves runtime
decisions and never registers definitions as a side effect of a data-plane
request. Keep its wire behavior aligned with
`contracts/openapi/flaggo-management-v1.yaml`.
