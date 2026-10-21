/* Node 24 loads the development tooling's authored TypeScript directly. */
require('./src/modules/development/index.ts').main().catch(error => {
  console.error(error.message);
  process.exitCode = 1;
});
