/* Browser export delivery: existing job polling and download handoff. */
(function () {
  function exportJson(response) {
    return response
      .json()
      .catch(function () {
        throw new Error('The export server answered ' + response.status + '.');
      })
      .then(function (result) {
        if (!response.ok || !result.ok) throw new Error(result.error || 'Export failed.');
        return result;
      });
  }

  function pollExport(job, startedAt, progress) {
    return fetch('/_workbench/export?job=' + encodeURIComponent(job.id), { cache: 'no-store' })
      .then(exportJson)
      .then(function (status) {
        progress(status, startedAt);
        if (status.status === 'failed') throw new Error(status.error || 'Export failed.');
        if (status.status === 'complete') return status;
        return new Promise(function (resolve) {
          setTimeout(resolve, 250);
        }).then(function () {
          return pollExport(job, startedAt, progress);
        });
      });
  }

  function downloadExport(job) {
    return fetch('/_workbench/export?job=' + encodeURIComponent(job.id) + '&download=1')
      .then(function (response) {
        if (!response.ok)
          return exportJson(response).then(function () {
            throw new Error('Export failed.');
          });
        var disposition = response.headers.get('Content-Disposition') || '';
        var match = /filename="([^"]+)"/.exec(disposition);
        return response.blob().then(function (blob) {
          return { blob: blob, name: match ? match[1] : 'canonic-design-system.zip' };
        });
      })
      .then(function (download) {
        var url = URL.createObjectURL(download.blob);
        var link = document.createElement('a');
        link.href = url;
        link.download = download.name;
        document.body.appendChild(link);
        link.click();
        link.remove();
        /* VS Code copies blob downloads through a temporary file. Keep the
           object URL alive while the editor completes that handoff. */
        setTimeout(function () {
          URL.revokeObjectURL(url);
        }, 60000);
        return download.name;
      });
  }
  function start(detail) {
    return fetch('/_workbench/export', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(detail),
    }).then(exportJson);
  }
  window.wbExport = { start: start, poll: pollExport, download: downloadExport };
})();
