# Fixture-backed Mock Server

The mock server replays the same golden cases used by contract conformance. It
does not implement decision logic. It requires the conformance dependencies in
`contracts\conformance\requirements.txt`.

Start it:

```powershell
python contracts\mock\fixture-server\server.py --port 8080
```

List selectable fixtures:

```powershell
curl.exe http://127.0.0.1:8080/_fixtures
```

Replay a case by sending its name in `X-Flaggo-Fixture` and reproducing the
method, path, required headers, and exact JSON body recorded by that fixture.
The server rejects malformed JSON, schema-invalid bodies, missing authorization
or content-type headers, and bodies that do not match the selected case.

```powershell
curl.exe -i -X POST `
  http://127.0.0.1:8080/v1/decisions/tetris.dropInterval:decide `
  -H "X-Flaggo-Fixture: decide-active-numeric-strategy" `
  -H "Authorization: Bearer local-test" `
  -H "Content-Type: application/json" `
  --data-binary "@request.json"
```

Here `request.json` must contain the fixture's recorded request body. The server
returns a problem response when the selected fixture and request do not match.
SDK-local and schema-negative cases are not replayable HTTP fixtures. Snapshot
fixtures are serialized with RFC 8785 JCS and replayed byte-for-byte so their
`ETag` and `Content-Digest` headers are verifiable across languages.
