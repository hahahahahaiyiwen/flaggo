// Generated from the decision manifest. Do not edit.
import type { RuntimeCatalog } from "@flaggo/sdk";

export const catalog = {
    "format": "flaggo.runtime-catalog/v1",
    "application": {
        "id": "tetris-demo",
        "environment": "dev"
    },
    "bundleDigest": "sha256:cf16b0e5ae310c0a674b0e58f6e7e52117d7c5651121cf00e9895ace206b74fb",
    "decisions": {
        "tetris.dropInterval": {
            "result": {
                "type": "number",
                "min": 200,
                "max": 1500,
                "step": 50,
                "default": 800
            },
            "context": {
                "userId": {
                    "type": "string",
                    "target": "user",
                    "required": false
                },
                "sessionId": {
                    "type": "string",
                    "target": "session",
                    "required": false
                },
                "cohort": {
                    "type": "string",
                    "target": "cohort",
                    "required": false
                },
                "deviceType": {
                    "type": "string",
                    "required": false
                }
            },
            "inputs": {
                "boardPressure": {
                    "source": "request",
                    "type": "number",
                    "meaning": "Current occupied-board fraction.",
                    "unit": "1",
                    "range": [0, 1]
                },
                "currentLevel": {
                    "source": "request",
                    "type": "number",
                    "meaning": "Current game level.",
                    "range": [0, 20]
                },
                "recentPlacementTimeMs": {
                    "source": "request",
                    "type": "number",
                    "meaning": "Application's current placement-time estimate.",
                    "unit": "ms",
                    "range": [0, 2000]
                },
                "recoveryFailures": {
                    "source": "request",
                    "type": "number",
                    "meaning": "Current recovery-failure count supplied by the game.",
                    "range": [0, 5]
                }
            },
            "contractDigest": "sha256:e36a037d3d26a5557f373a984ddb70a9eb26a78fb197778e70972fea444b7d45"
        }
    }
} as const satisfies RuntimeCatalog;
