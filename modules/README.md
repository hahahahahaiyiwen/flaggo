# Domain Modules

Each directory is a business capability with its own interfaces, invariants,
tests, and documentation. Modules collaborate through constructor-injected
ports owned by the module where the behavior belongs.

Infrastructure adapters may implement these ports in application projects or
future adapter packages. Do not create a global interfaces project.
