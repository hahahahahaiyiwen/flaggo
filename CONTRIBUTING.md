# Contributing to Flaggo

Flaggo is early-stage. Design clarity, executable contracts, and a portable
local workflow take priority over implementation volume.

## Development setup

Requirements:

- Python 3.11 or newer
- Git
- Docker with Compose (optional)

Install the current contract tooling and run the baseline:

```powershell
python -m pip install -r contracts\conformance\requirements.txt
python tools\dev.py check
```

Start the fixture-backed API:

```powershell
python tools\dev.py serve
```

No cloud account or external service is required.

## Change expectations

- Update executable contracts and fixtures when wire behavior changes.
- Keep module interfaces owned by the module where behavior belongs.
- Use constructor injection for cross-module dependencies.
- Add or update tests before implementation behavior changes.
- Update a module's `README.md` when its boundary, invariants, or dependencies
  change.
- Keep commits focused and avoid unrelated formatting or generated output.

## Pull requests

Describe the behavior changed, the boundary that owns it, and the validation
performed. Contract changes must keep OpenAPI, schemas, fixtures, docs, and the
conformance gate aligned.
