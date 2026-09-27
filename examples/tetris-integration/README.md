# Interactive Tetris and Flaggo integration

This example is a playable terminal Tetris application. The game owns its
board, pieces, controls, scoring, levels, and gravity scheduling. It runs
standalone with a local gravity policy and can optionally ask Flaggo for the
drop interval.

## Play standalone

From the repository root:

```powershell
npm run tetris
```

This build contains only the game engine, terminal UI, and local
`DropIntervalProvider`. It does not import the Flaggo SDK or require either
Flaggo service.

## Play with local Flaggo services

Install the npm dependencies and .NET 10 SDK, then run:

```powershell
npm run tetris:flaggo
```

The command builds and starts Contract Service and Decision Service against an
isolated SQLite database, then deploys the contracts listed in
`flaggo.deploy.json`. It binds the returned immutable contract digest and
starts the game. Both services and the temporary database are stopped and
removed when the game exits.

The game requests a new interval at startup and after each piece locks, not on
every gravity tick. Its status line shows whether the current interval came
from Flaggo, the standalone policy, or the standalone policy after a Flaggo
failure.

If Flaggo becomes unavailable, the app keeps running with local level-based
gravity and displays `Local fallback`. This is application behavior implemented
by the optional provider; the SDK does not synthesize fallback decisions.

## Controls

| Action | Keys |
| --- | --- |
| Move left or right | Left/Right arrows or `A`/`D` |
| Rotate clockwise | Up arrow or `W` |
| Soft drop | Down arrow or `S` |
| Hard drop | Space |
| Pause or resume | `P` |
| Restart | `R` |
| Quit | `Q` or Ctrl+C |

The board is 10x20 and uses a seven-bag piece source. The engine supports all
seven tetrominoes, clockwise rotation with simple horizontal kicks, line
clearing, scoring, levels, soft drop, hard drop, pause, restart, and game over.

## Decision contract

`flaggo/contracts/tetris.dropInterval.decision-contract.json` defines
`tetris.dropInterval`. The file name must preserve the exact decision name and
end in `.decision-contract.json`; `flaggo.deploy.json` references the file.

| Condition | Interval |
| --- | --- |
| Board pressure is at least `0.8`, or recovery failures are at least `3` | `850ms` |
| Lower pressure and fewer than `3` recovery failures | `750ms` |
| No authored rule is eligible | Contract default `800ms` |

The optional adapter uses `createDecisionClient` from `@flaggo/sdk/runtime`.
The deployment helper uses `createContractClient` from
`@flaggo/sdk/management` after both local services are ready. Its `deploy`
operation sends the authoritative `PUT`; it does not call the optional remote
validation endpoint first.

## Automated checks

Run deterministic engine, provider, renderer, and terminal-cleanup tests:

```powershell
npm run test:tetris-app
```

Run the real-host integration independently:

```powershell
npm run test:tetris-integration
```

The real-host harness verifies the `850`, `750`, and `800` paths, SDK-owned
`_random`, exact digest provenance, retired-field absence, and explicit
failure after Decision Service stops.
