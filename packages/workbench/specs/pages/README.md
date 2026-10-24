# Pages and implementations

This folder owns page authoring and page-specific user workflows. Start with
[core](../core.md) for the merged page model, routing and shared failures, and
[lenses](../canvas/lenses.md) for stable lens identity and labels.

| Topic | Owns |
| --- | --- |
| [TypeScript previews](previews.md) | Definition API, discovery semantics, scenarios, inputs/actions, request mocks, navigation and portable viewer workflows |
| [Docs](docs-pages.md) | Markdown, docs lenses, example/source panels, page lifecycle, reading presentation and Docs compatibility |
| [Authored pages and implementations](implementations.md) | Authored HTML, URL/native mappings, source pointers, compatibility and native-window interaction |
| [Storybook pages](storybook.md) | Title/story identity, selected-story navigation, channel reuse, capture/export mapping and failure behavior |
| [Configure pages](configuration-editor.md) | Authored collection drafts, save scope, YAML preservation, refresh, write failures and concurrent-edit gaps |

The [pipeline](../preview-pipeline/README.md) owns shared discovery mechanics,
compilation, delivery, update transport, retained documents and readiness.
The [adapters](../adapters/README.md) own format-specific build and mount differences.
For example, Storybook's process and socket belong to its adapter, while story
selection belongs here. Docs owns start/stop state within a page; the warm Docs
host owns reuse and isolation between destinations.

Feature integration sections describe how a page participates in canvas/review
workflows. They link to [sizes](../canvas/sizes.md),
[persistence](../canvas/persistence.md) and [review](../review/README.md) rather
than introducing separate defaults, storage or capture policies.
