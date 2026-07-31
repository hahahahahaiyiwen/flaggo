# Applications

`apps` contains independently runnable Flaggo hosts. Applications compose
module-owned interfaces with infrastructure adapters; domain modules never
depend on application projects.

Initial hosts are the control plane, data plane, and operator console. A future
worker may be added when asynchronous intelligence requires an independent
process.
