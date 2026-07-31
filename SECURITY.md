# Security Policy

Flaggo is pre-release and does not yet publish supported release lines.

Do not open public issues for suspected vulnerabilities. Use GitHub's private
vulnerability reporting for this repository when available. Never include
credentials, production data, or other secrets in reports, fixtures, examples,
or commits.

Security-sensitive contract behavior fails closed by default. Contract identity
errors, authorization failures, and policy blocks must not be converted into
successful decisions or silent local fallback.
