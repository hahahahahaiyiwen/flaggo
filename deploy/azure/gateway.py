"""Refresh the gateway's real systemd PID metadata after backend restarts."""

import json
import os
import signal
import subprocess
import time

SERVICES = [
    "contract-service", "decision-service", "otel-ingestion",
    "evidence-materializer", "async-analysis",
]


def service_pids():
    analysis = os.environ["FLAGGO_ANALYSIS_UNIT"]
    if analysis not in {"flaggo@async-analysis.service", "flaggo-smoke-analysis.service"}:
        raise ValueError("Select exactly one supported analysis unit")
    result = {}
    for service in SERVICES:
        unit = analysis if service == "async-analysis" else f"flaggo@{service}.service"
        pid = int(subprocess.check_output(
            ["systemctl", "show", unit, "--property=MainPID", "--value"], text=True,
        ).strip())
        if pid < 0:
            raise ValueError(f"Invalid MainPID for {unit}")
        result[f"flaggo-{service}"] = pid
    return result


class Gateway:
    def __init__(self, start):
        self.start = start
        self.child = None
        self.pids = None

    def stop(self):
        if self.child is not None and self.child.poll() is None:
            self.child.terminate()
            try:
                self.child.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait()
        self.child = None

    def refresh(self, pids):
        if self.child is not None and self.child.poll() is not None:
            raise RuntimeError(f"Gateway exited unexpectedly: {self.child.returncode}")
        if not all(pids.values()) or self.pids == pids:
            return
        self.stop()
        print("Starting gateway with refreshed systemd PID metadata", flush=True)
        self.child = self.start({
            **os.environ, "FLAGGO_PLAYGROUND_SERVICE_PROCESSES": json.dumps(pids),
        })
        self.pids = pids.copy()


def main():
    def terminate(_signal, _frame):
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, terminate)
    gateway = Gateway(lambda environment: subprocess.Popen([
        "/opt/flaggo/current/node/bin/node",
        "/opt/flaggo/current/examples/tetris-playground/dist/server.js",
    ], env=environment))
    try:
        while True:
            gateway.refresh(service_pids())
            time.sleep(1)
    finally:
        gateway.stop()


if __name__ == "__main__":
    main()
