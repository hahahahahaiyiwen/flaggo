# Policy Module

Owns effective-policy resolution and safety evaluation for proposed or runtime
decisions.

Policy results are explicit domain types with stable reason codes. A blocked
candidate cannot be returned as approved, and policy never performs rollout or
storage operations directly. Collaborators are constructor-injected async
ports.

Update this document when policy composition, constraints, or fallback
authority changes.
