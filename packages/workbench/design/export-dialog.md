# Export

Choose the scope and format before starting a download. Source packages and portable viewers include declared variants; single-page images and PDFs default to the current view.

## Current page

```example current-page
caption: Open the export dialog for the current page.
```

## Public API

Importing `WbExportDialog` does not register an element. Browser entry points call
`registerCanvasComponents()` from `ui/src/bootstrap/register.js` once per document.

`wb-export-dialog` accepts `pages` (ID and label), `collections` (names), and optional `current` (page, state, lens, size, width, height). `show()` opens its native modal. It emits the composed, bubbling `wb-export-request` event with the chosen options. The host owns job progress and downloads. Cancel and Escape close the modal without a request. Disconnect closes it and removes listeners.

The page list permits multiple selections. PDF documentation paper can be A4 or Letter. Image output can be PNG or JPEG.
