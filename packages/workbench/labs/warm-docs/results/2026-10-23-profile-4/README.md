# Warm docs profiling pass: profile-4

Started 2026-10-23T03:54:30.811Z, finished 2026-10-23T03:58:33.979Z. One measured session per variant and fixture, traced; medians per cell. Diagnostic, not a timing comparison.

## acme, control

Chrome counters per step (Performance.getMetrics deltas), median: task, script, layout and style ms; layouts, style recalculations and documents created.

| Cell | n | Visible | Task | Script | Layout | Style | Layouts | Recalcs | Documents |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| first | 16 | 130 | 95 | 16 | 10 | 17 | 22 | 29 | 3 |
| evicted | 8 | 126 | 84 | 17 | 9 | 16 | 21 | 29 | 3 |
| retained | 8 | 28 | 20 | 6 | 2 | 2 | 11 | 18 | 0 |
| design-return | 2 | 19 | 19 | 6 | 2 | 2 | 12 | 23 | 0 |
| edit-markdown | 2 | 62 | 23 | 2 | 3 | 3 | 2 | 10 | 2 |
| edit-example | 2 | 140 | 157 | 14 | 11 | 21 | 12 | 31 | -5 |

Main thread in each navigation window (selection → visible), from the trace, median ms:

| Cell | n | Window | Busy | Waiting | scripting | html | style | forced layout | layout | paint | loading | document setup | gc | fonts | other | Worker | Frames |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| first | 16 | 131 | 86 | 46 | 27 | 2 | 15 | 0 | 10 | 5 | 7 | 0 | 2 | 0 | 10 | 6 | 11 |
| evicted | 8 | 127 | 75 | 41 | 28 | 1 | 15 | 0 | 9 | 4 | 7 | 0 | 1 | 0 | 10 | 5 | 10 |
| retained | 8 | 29 | 15 | 15 | 8 | 0 | 2 | 0 | 2 | 1 | 1 | 0 | 0 | 0 | 1 | 0 | 4 |
| design-return | 2 | 20 | 14 | 5 | 8 | 0 | 2 | 0 | 2 | 1 | 1 | 0 | 0 | 0 | 1 | 1 | 5 |
| edit-markdown | 2 | 63 | 7 | 52 | 3 | 0 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | 0 | 2 | 1 | 4 |
| edit-example | 2 | 140 | 54 | 86 | 19 | 1 | 8 | 0 | 5 | 3 | 7 | 0 | 1 | 0 | 11 | 8 | 10 |

Scripting by script, mean ms per new-page navigation: (no script url) 14.3; /_workbench/ui/src/modules/preview-sessions/renderer-bridge.js 5.0; /_workbench/content-frames.js 4.3; /_workbench/workbench.js 2.6; /_workbench/ui/src/modules/preview-sessions/runtime.js 0.6; /_workbench/preview-compat.js 0.4; /_workbench/top-bar.js 0.2; project chunks (runtime) 0.1.

Phases after selection (new pages), median ms: selection 4, renderer-ready 7, document-script 56, document-loaded 57, bundle-info-fetched 63, bundle-fetched 79, examples-mounted 82, readiness-environment 82, readiness-fonts 106, readiness-images 107, readiness 108, frame-promotion 110, visible 133, visible 126.

| Request (new pages) | Per navigation | Starts at | Server response (TTFB) | Duration |
| --- | --- | --- | --- | --- |
| docs page | 1 | 25 | 4.9 | 11 |
| workbench script | 8 | 52 | 1.0 | 3 |
| bundle answer | 1 | 57 | 5.5 | 7 |
| page module | 1 | 63 | 1.0 | 3 |
| runtime chunk | 1 | 71 | 2.0 | 0 |
| chunk | 4 | 72 | 2.1 | 0 |
| module stylesheet | 1 | 72 | 0.9 | 3 |
| font | 4 | 81 | 2.7 | 0 |
| capture | 1 | 99 | 5.6 | 10 |
| other | 1 | 164 | 2.7 | 6 |

Mount hooks, median ms: setup 0.0, each example 0.1, ready 0.0, fonts 19.7, images 0.1; all examples of a page 1.

Show code (first open, then reopened): shown 21, coloured 78; shown 0, coloured 1; shown 0, coloured 0.
Scrolling frames: canvas p50 16.7, p95 16.7, max 16.8 ms, 0 of 91 frames over 20 ms; document p50 16.7, p95 16.8, max 16.8 ms, 0 of 91 frames over 20 ms.
Zooming frames: canvas p50 16.7, p95 16.7, max 16.8 ms, 0 of 181 frames over 20 ms; document p50 16.7, p95 16.7, max 16.8 ms, 0 of 181 frames over 20 ms.

## acme, pool

Chrome counters per step (Performance.getMetrics deltas), median: task, script, layout and style ms; layouts, style recalculations and documents created.

| Cell | n | Visible | Task | Script | Layout | Style | Layouts | Recalcs | Documents |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| first | 16 | 106 | 52 | 9 | 6 | 6 | 16 | 28 | 1 |
| evicted | 8 | 97 | 46 | 8 | 5 | 5 | 15 | 23 | 1 |
| retained | 8 | 70 | 25 | 7 | 2 | 2 | 11 | 19 | 0 |
| design-return | 2 | 99 | 27 | 8 | 2 | 3 | 13 | 28 | -3 |
| edit-markdown | 2 | 58 | 26 | 2 | 3 | 3 | 2 | 12 | 2 |
| edit-example | 2 | 164 | 164 | 13 | 12 | 22 | 10 | 33 | -2 |

Main thread in each navigation window (selection → visible), from the trace, median ms:

| Cell | n | Window | Busy | Waiting | scripting | html | style | forced layout | layout | paint | loading | document setup | gc | fonts | other | Worker | Frames |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| first | 16 | 107 | 45 | 57 | 20 | 1 | 5 | 0 | 6 | 4 | 4 | 0 | 0 | 0 | 5 | 2 | 9 |
| evicted | 8 | 97 | 41 | 55 | 20 | 1 | 4 | 0 | 5 | 3 | 4 | 0 | 0 | 0 | 4 | 2 | 9 |
| retained | 8 | 71 | 20 | 51 | 10 | 0 | 2 | 0 | 2 | 2 | 2 | 0 | 0 | 0 | 3 | 1 | 7 |
| design-return | 2 | 100 | 23 | 65 | 11 | 0 | 2 | 0 | 2 | 2 | 2 | 0 | 0 | 0 | 3 | 2 | 9 |
| edit-markdown | 2 | 59 | 8 | 51 | 4 | 0 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | 0 | 2 | 1 | 5 |
| edit-example | 2 | 164 | 66 | 93 | 25 | 1 | 8 | 0 | 5 | 6 | 7 | 0 | 1 | 0 | 12 | 12 | 11 |

Scripting by script, mean ms per new-page navigation: (no script url) 13.3; /_workbench/workbench.js 2.7; /_workbench/ui/src/modules/preview-sessions/renderer-bridge.js 2.5; /_workbench/content-frames.js 1.5; /_workbench/ui/src/modules/preview-sessions/runtime.js 0.6; /_workbench/top-bar.js 0.2; /_workbench/ui/src/modules/preview-sessions/story-catalog.js 0.1; /_workbench/preview-controls.js 0.1.

Phases after selection (new pages), median ms: selection 4, renderer-ready 8, document-loaded 63, document-script 62, bundle-info-fetched 74, bundle-fetched 80, examples-mounted 81, readiness-environment 82, readiness-fonts 88, readiness-images 88, readiness 89, frame-promotion 92, visible 107, visible 106.

| Request (new pages) | Per navigation | Starts at | Server response (TTFB) | Duration |
| --- | --- | --- | --- | --- |
| docs page | 1 | 9 | 6.2 | 40 |
| bundle answer | 1 | 62 | 6.3 | 11 |
| page module | 1 | 74 | 1.2 | 3 |
| workbench script | 10 | 76 | 0.9 | 4 |
| chunk | 1 | 83 | 1.1 | 0 |
| capture | 1 | 95 | 7.4 | 11 |
| runtime chunk | 1 | 124 | 1.8 | 0 |
| module stylesheet | 1 | 124 | 0.9 | 4 |
| font | 4 | 131 | 2.9 | 0 |
| other | 1 | 162 | 2.8 | 4 |

Mount hooks, median ms: setup 0.0, each example 0.1, ready 0.0, fonts 5.2, images 0.1; all examples of a page 1.

Show code (first open, then reopened): shown 21, coloured 86; shown 0, coloured 1; shown 1, coloured 1.
Scrolling frames: canvas p50 16.7, p95 16.8, max 33.4 ms, 2 of 88 frames over 20 ms; document p50 16.7, p95 16.7, max 33.4 ms, 2 of 88 frames over 20 ms.
Zooming frames: canvas p50 16.7, p95 16.7, max 16.8 ms, 0 of 182 frames over 20 ms; document p50 16.7, p95 16.7, max 16.8 ms, 0 of 182 frames over 20 ms.

## heavy, control

Chrome counters per step (Performance.getMetrics deltas), median: task, script, layout and style ms; layouts, style recalculations and documents created.

| Cell | n | Visible | Task | Script | Layout | Style | Layouts | Recalcs | Documents |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| first | 29 | 148 | 100 | 28 | 5 | 4 | 23 | 29 | 3 |
| evicted | 13 | 149 | 98 | 28 | 5 | 4 | 23 | 29 | 3 |
| retained | 11 | 28 | 21 | 7 | 1 | 2 | 11 | 18 | 0 |
| lens-switch | 4 | 32 | 41 | 7 | 1 | 2 | 12 | 19 | 0 |
| edit-markdown | 2 | 80 | 86 | 7 | 2 | 2 | 2 | 2 | 2 |
| edit-example | 2 | 143 | 96 | 10 | 0 | 0 | 2 | 2 | 0 |


| Cell | n | Window | Busy | Waiting | scripting | html | style | forced layout | layout | paint | loading | document setup | gc | fonts | other | Worker | Frames |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |

Show code (first open, then reopened): shown 27, coloured 61; shown 1, coloured 1; shown 1, coloured 1.
Scrolling frames: canvas p50 16.7, p95 16.8, max 16.8 ms, 0 of 90 frames over 20 ms; document p50 16.7, p95 16.7, max 16.8 ms, 0 of 90 frames over 20 ms.
Zooming frames: canvas p50 16.7, p95 16.8, max 16.8 ms, 0 of 181 frames over 20 ms; document p50 16.7, p95 16.7, max 16.8 ms, 0 of 181 frames over 20 ms.
Heap over a 50-page walk, MB after GC: start 12.7, after 5 12.4, middle 13.0, end 12.8.

## heavy, pool

Chrome counters per step (Performance.getMetrics deltas), median: task, script, layout and style ms; layouts, style recalculations and documents created.

| Cell | n | Visible | Task | Script | Layout | Style | Layouts | Recalcs | Documents |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| first | 29 | 93 | 67 | 15 | 4 | 5 | 17 | 24 | 1 |
| evicted | 13 | 93 | 62 | 15 | 3 | 4 | 16 | 23 | 1 |
| retained | 11 | 43 | 26 | 7 | 1 | 2 | 12 | 19 | 0 |
| lens-switch | 4 | 44 | 28 | 8 | 1 | 2 | 12 | 19 | 0 |
| edit-markdown | 2 | 78 | 84 | 7 | 2 | 2 | 2 | 2 | 2 |
| edit-example | 2 | 161 | 99 | 11 | 0 | 0 | 2 | 2 | 0 |


| Cell | n | Window | Busy | Waiting | scripting | html | style | forced layout | layout | paint | loading | document setup | gc | fonts | other | Worker | Frames |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |

Show code (first open, then reopened): shown 25, coloured 59; shown 0, coloured 1; shown 0, coloured 1.
Scrolling frames: canvas p50 16.7, p95 16.8, max 16.8 ms, 0 of 91 frames over 20 ms; document p50 16.7, p95 16.7, max 16.7 ms, 0 of 91 frames over 20 ms.
Zooming frames: canvas p50 16.7, p95 16.7, max 16.8 ms, 0 of 181 frames over 20 ms; document p50 16.7, p95 16.7, max 16.8 ms, 0 of 181 frames over 20 ms.
Heap over a 50-page walk, MB after GC: start 19.3, after 5 19.3, middle 21.5, end 23.9.
