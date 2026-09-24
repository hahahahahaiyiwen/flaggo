// Generated from the decision manifest. Do not edit.
import type { RuntimeCatalog } from "@flaggo/sdk";

export const catalog = {
    "format": "flaggo.runtime-catalog/v1",
    "application": {
        "id": "adaptive-worker-demo",
        "environment": "dev"
    },
    "bundleDigest": "sha256:3c85fdb1b39a8b31a53dcea02e43afab46de5ab4ed323676ff3dcb59745937ea",
    "decisions": {
        "demo.workerBatchSize": {
            "result": {
                "type": "number",
                "min": 1,
                "max": 10,
                "step": 1,
                "default": 3
            },
            "context": {
                "workerId": {
                    "type": "string",
                    "required": true
                },
                "cohort": {
                    "type": "string",
                    "target": "cohort",
                    "required": false
                }
            },
            "inputs": {
                "queuePressure": {
                    "source": "request",
                    "type": "number",
                    "meaning": "Current normalized queue pressure computed by the worker.",
                    "unit": "1",
                    "range": [0, 1]
                }
            },
            "contractDigest": "sha256:c2ef39d61ac1e64365925a99913107c0062c21336107c6259c96b6da915ed4c1"
        }
    }
} as const satisfies RuntimeCatalog;
