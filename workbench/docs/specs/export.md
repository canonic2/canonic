# Design-system export contract

The design-system ZIP gathers the workbench's source closure and reference
screenshots for its screens. It is a server-backed operation available from
the editor or a hosted browser. See [server.js](../../server.js),
[export.js](../../export.js), and [electron-capture.js](../../electron-capture.js).

## Scope and job lifecycle

- Export starts from every design file and resolved component, page, and
  Storybook source pointer, follows reachable local imports and assets, and
  keeps project-relative paths. It includes relevant Storybook and package
  configuration. Installed dependencies, build output, secrets, tests, and
  unrelated files are excluded.
- `POST /_workbench/export` starts a background job. Its status endpoint
  reports progress and a completed job can be downloaded. A legacy direct
  `GET` waits for completion and downloads. The workbench shows modal
  progress, pauses speculative interactive preparation, and leaves the
  visible selection and preview untouched.
- The plan includes every declared design state and each imported Storybook
  story at supported viewports. `responsive` expands to desktop and mobile;
  duplicate sizes are removed. The standard `fit` reference is 1440 × 900.
  Implementation-only Simulator entries and unsupported or missing references
  produce warnings rather than invented images.

## Capture scheduling versus the camera

| | Interactive camera and handoff | Design-system export |
| --- | --- | --- |
| Target | Current visible selection, scroll, annotations, and live document state | Planned design states and imported Storybook stories at configured sizes |
| Preparation | Continuous live mirror into one warm view; flush on click | Direct background loads and settling for each planned reference |
| Renderer use | Single queued helper | Up to four workers for distinct page/story groups; the existing warm service can be one worker |
| Reuse | Reprepare the selected view | Keep all viewport sizes of one URL on its worker; switch Storybook stories in a loaded `iframe.html` runtime when possible |
| Result | Browser JPEG download or saved handoff screenshot | JPEG references embedded in the ZIP |

Export calls the same capture service but has its own jobs and scheduling.
References for one page or story stay on one worker to reuse the loaded
document across viewport sizes. Other groups can run in parallel. Storybook
reuse inside a worker does not navigate the visible iframe. A failed capture
adds a `captureWarnings` entry and the remaining references continue.

## Archive contract

- Each screen gets an agent-oriented README beside its primary source entry
  and JPEGs in a nearby `screenshots/` directory. Shared source directories
  receive screen-specific names. The README points to its reference images.
- `canonic-export.json` lists files, dependencies, unresolved references,
  screen records, warnings, and `captureWarnings`. Each screen record has a
  SHA-256 content hash over its sorted exported source paths and raw file
  contents. Generated metadata and reference JPEGs do not affect that hash.
- A small export can be one ZIP. A larger export downloads an outer
  `<design-system>-parts.zip` containing numbered ZIPs of at most 10,000,000
  bytes each. Parts extract into the same top-level directory and repeat the
  top README and export report; each part has its own file inventory.

## Verification points

- [server.test.js](../../server.test.js) checks capture plans, jobs, progress,
  and warnings.
- [export.test.js](../../export.test.js) checks source closure, screen hashes,
  screenshot placement, and archive parts.
- [electron-capture.test.js](../../electron-capture.test.js) and
  [capture.test.js](../../capture.test.js) check renderer reuse and fallbacks.
