// Runs a scenario's tool stub in place of the real program and logs the call for
// the oracle. Usage (from a generated shim): tool-runner.mjs <module> <name> <env-prefix> ...args
import { appendFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const [module, name, prefix, ...args] = process.argv.slice(2);
const environment = prefix ? Object.fromEntries(Object.entries(process.env).filter(([key]) => key.startsWith(prefix))) : {};
const stub = (await import(pathToFileURL(module).href)).default;
const result = (await stub(args, { env: process.env, cwd: process.cwd() })) ?? {};
const code = result.code ?? 0;
if (process.env.BENCHMARK_TOOL_LOG) {
  appendFileSync(process.env.BENCHMARK_TOOL_LOG,
    `${JSON.stringify({ at: new Date().toISOString(), tool: name, args, cwd: process.cwd(), environment, code, effective: result.effective ?? null })}\n`);
}
if (result.stdout) process.stdout.write(result.stdout);
if (result.stderr) process.stderr.write(result.stderr);
process.exitCode = code;
