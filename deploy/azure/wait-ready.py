"""Wait for actual HTTP readiness; fail explicitly after a bounded interval."""

import sys
import time
import urllib.error
import urllib.request


def main():
    if len(sys.argv) < 2:
        raise ValueError("At least one readiness URL is required")
    for url in sys.argv[1:]:
        for attempt in range(30):
            try:
                with urllib.request.urlopen(url, timeout=2) as response:
                    if response.status != 200:
                        raise RuntimeError(f"{url} returned HTTP {response.status}")
                    break
            except (urllib.error.URLError, TimeoutError) as error:
                if attempt == 29:
                    raise RuntimeError(f"Readiness did not succeed: {url}") from error
                time.sleep(1)


if __name__ == "__main__":
    main()
