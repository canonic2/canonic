/* The docs sidebar's groups, in order, by guide id. Plain data, so the docs
   preview's definition, which also runs in the browser, can read it. */
export const docGroups = [
  { title: 'Getting started', ids: ['index', 'getting-started', 'pages-and-scenarios', 'docs-pages', 'canvas', 'annotations-and-handoff', 'spaces'] },
  { title: 'Previews', ids: ['workbench-previews', 'preview-data', 'react', 'react-native-web', 'vue', 'astro', 'html', 'custom-adapters'] },
  { title: 'Connect your implementation', ids: ['lenses', 'storybook', 'agent-tools'] },
  { title: 'Reference', ids: ['configuration', 'design-system-export', 'extension', 'troubleshooting'] },
];

export const testrunDocGroups = [
  { title: 'Get started', ids: ['index', 'getting-started', 'distribution'] },
  { title: 'Test apps and browsers', ids: ['ios', 'android', 'browser', 'builds', 'projects', 'playback', 'viewer-control'] },
  { title: 'Use the CLI and MCP', ids: ['cli', 'mcp', 'configuration', 'pagination'] },
  { title: 'Investigate failures', ids: ['debugging', 'diagnostic-access', 'diagnostic-workflows', 'diagnostic-reproduction', 'diagnostic-reports', 'diagnostic-testing', 'diagnostic-interactive'] },
  { title: 'Platform and framework diagnostics', ids: ['diagnostic-android', 'diagnostic-android-state', 'diagnostic-apple', 'diagnostic-browser', 'diagnostic-frameworks', 'diagnostic-react-native', 'diagnostic-dart', 'diagnostic-services', 'diagnostic-observability'] },
  { title: 'Reference', ids: ['diagnostic-formats', 'diagnostic-performance', 'diagnostic-native-traces', 'diagnostic-memory', 'diagnostic-network-data', 'diagnostic-scenario-context', 'troubleshooting'] },
];

export const shieldDocGroups = [
  { title: 'Get started', ids: ['index', 'introduction', 'getting-started', 'policy-decisions', 'operator-experience'] },
  { title: 'Work with your project', ids: ['policy-files', 'project-review', 'local-registry', 'claude-project', 'mcp-connections', 'troubleshooting'] },
  { title: 'Write rules', ids: ['policy-rules', 'rule-reference', 'pack-validation'] },
  { title: 'Integrate Shield', ids: ['project-mcp', 'claude-read-only', 'native-run', 'enforcement-handoff'] },
  { title: 'Recovery and development', ids: ['compiled-cache', 'development'] },
];
