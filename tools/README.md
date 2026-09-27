# Development Tools

Repository tooling provides stable, cloud-independent entry points for local
validation and fixture-server development.

```powershell
python tools\dev.py check
python tools\dev.py serve
```

`check` runs the executable-contract gate. `serve` starts the fixture-only
contract server; it does not start Contract Service or Decision Service.
Tooling must surface failures directly.
