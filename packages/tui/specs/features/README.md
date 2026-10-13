# Feature specifications

Features describe workflows and capabilities that compose components or serve
multiple components. Each feature owns its behavior, boundaries and acceptance;
[component contracts](../components/README.md) own individual reusable views.

| Feature | Owns | Current status |
| --- | --- | --- |
| [Gallery](gallery.md) | Component → scenario navigation, retained picker state, deterministic fixtures, CLI and verification | Implemented local gallery; broader targets documented |
| [Gallery scenario coverage](gallery-designs.md) | Runnable scenario inventory and pending coverage by component | Coverage map for the gallery |
| [Input, focus and navigation](interaction.md) | Routing, focus scopes, keyboard bindings, scrolling, selection and copy | Focus and keyboard foundations implemented; pointer interaction proposed |
| [Activity, review and cancellation](activity.md) | Tool/run state, permissions, questions and cancellation | Cards/groups, Run Activity, Question Form, Permission Review and shared queue implemented; host authority remains external |
| [Browsing, panels and notices](browsing.md) | Session/resource browsing, trees, summary panels and notices | Session/Tree Browser, Code/Diff Inspection and Status Panels & Notices implemented; other browsers proposed |
| [Terminal hosting and compatibility](terminal.md) | Terminal lease, inline/full-screen hosting, print ledger, cleanup and accessibility | Bounded inline/full-screen leases and foreground handoff implemented; wider native acceptance pending |

These public contracts contain their implementation requirements without links
to private design files. Package-wide architecture, delivery and engineering
checks remain in the [specifications index](../README.md).

## Contract ownership

A feature states scope/status, workflow and participating components, host
boundaries, failure/recovery behavior, acceptance and remaining decisions. Link
to component behavior instead of defining a second editor, picker or modal
contract. Gallery owns navigation and fixture lifecycle; its coverage map is an
inventory of scenarios, not a separate requirement owner. Shared keyboard rules
belong to Input/navigation; terminal leases belong to Terminal hosting.
Executed checks and reported human acceptance are tracked in
[Verification](../verification.md), independently of feature implementation.
