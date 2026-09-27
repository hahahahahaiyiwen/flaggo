# Development Tools

Repository tooling provides stable, cloud-independent entry points for local
validation and fixture-backed API development.

```powershell
python tools\dev.py check
python tools\dev.py serve
```

`check` runs the executable-contract gate. `serve` starts the fixture-backed
development API. Tooling must surface failures directly.
