/* The docs sidebar's groups, in order, by guide id. Plain data, so the docs
   preview's definition, which also runs in the browser, can read it. */
export const docGroups = [
  { title: 'Getting started', ids: ['index', 'getting-started', 'pages-and-states', 'canvas', 'markup-and-handoff', 'projects'] },
  { title: 'Previews', ids: ['workbench-previews', 'preview-data', 'react', 'react-native-web', 'vue', 'astro', 'html', 'custom-adapters'] },
  { title: 'Connect your implementation', ids: ['lenses', 'storybook', 'ios-simulator', 'windows'] },
  { title: 'Reference', ids: ['configuration', 'design-system-export', 'extension', 'troubleshooting'] },
];
