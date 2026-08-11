export async function waitForServiceShutdown(
  lifecycle,
  { pollIntervalMilliseconds = 100 } = {},
) {
  if (
    !Number.isInteger(pollIntervalMilliseconds) ||
    pollIntervalMilliseconds < 1
  ) {
    throw new TypeError(
      "pollIntervalMilliseconds must be a positive integer.",
    );
  }

  while (!lifecycle.signal.aborted) {
    lifecycle.assertHealthy();
    await waitForNextCheck(
      lifecycle.signal,
      pollIntervalMilliseconds,
    );
  }
}

function waitForNextCheck(signal, milliseconds) {
  if (signal.aborted) return Promise.resolve();
  return new Promise((resolvePromise) => {
    const timer = setTimeout(finish, milliseconds);
    signal.addEventListener("abort", finish, { once: true });

    function finish() {
      clearTimeout(timer);
      signal.removeEventListener("abort", finish);
      resolvePromise();
    }
  });
}
