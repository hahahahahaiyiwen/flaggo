import { LocalDropIntervalProvider } from "./drop-interval.js";
import { runTerminalTetris } from "./terminal.js";

export async function runMain(): Promise<void> {
  const controller = new AbortController();
  const abort = (): void => controller.abort();
  process.once("SIGINT", abort);
  process.once("SIGTERM", abort);
  try {
    await runTerminalTetris({
      provider: new LocalDropIntervalProvider(),
      signal: controller.signal,
    });
  } finally {
    process.removeListener("SIGINT", abort);
    process.removeListener("SIGTERM", abort);
  }
}

try {
  await runMain();
} catch (error) {
  process.exitCode = 1;
  process.stderr.write(
    `${error instanceof Error ? error.message : String(error)}\n`,
  );
}
