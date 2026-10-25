# Capture, handoff and export

This folder owns visual review outputs and their acquisition lifecycle.
The [canvas](../canvas/README.md) supplies selected views or artboard snapshots;
review must not change the user's selection to produce an output.

| Topic | Owns |
| --- | --- |
| [Interactive capture](capture.md) | Pixel acquisition, live mirroring, bundled helper, camera/handoff outputs, limits and failures |
| [Design-system export](export.md) | Page/collection/space scopes, capture plan, jobs, source closure, PDF rendering and portable/PDF/image archives, warnings and retention; export sizes are in [sizes](../canvas/sizes.md#design-system-export) |

Interactive review captures supplied live state. Catalog export schedules its
own declared references and background workers. Their shared helper does not
make their target, scheduling or retention policies interchangeable.

[Agent context](../integrations/agent-context.md) owns reference delivery to
chats and tools; [artboard composition](../canvas/multiple-artboards.md) owns
the region model. [Server](../system/server.md) owns host/process transport,
and [readiness](../preview-pipeline/readiness.md) owns content readiness.
