// Generated from the decision manifest. Do not edit.
import type { RuntimeCatalog } from "@flaggo/sdk";

export const catalog = {
    "format": "flaggo.runtime-catalog/v1",
    "application": {
        "id": "otel-worker",
        "environment": "dev"
    },
    "bundleDigest": "sha256:46abb3578a8da367f3e1e069663c790ffaee82e07029cf526168573c71c9ad18",
    "decisions": {
        "worker.batchSize": {
            "result": {
                "type": "number",
                "min": 1,
                "max": 10,
                "step": 1,
                "default": 3
            },
            "context": {
                "sessionId": {
                    "type": "string",
                    "target": "session",
                    "required": true
                }
            },
            "inputs": {
                "pressure": {
                    "source": "evidence",
                    "binding": "queuePressure"
                },
                "durationMs": {
                    "source": "evidence",
                    "binding": "processingTime"
                },
                "retries": {
                    "source": "evidence",
                    "binding": "retryCount"
                },
                "failures": {
                    "source": "evidence",
                    "binding": "failureCount"
                }
            },
            "contractDigest": "sha256:f85e212527abd2c535fccb731b8e894ebc8b5deb9cba416778d19e5babf009f7"
        }
    }
} as const satisfies RuntimeCatalog;
