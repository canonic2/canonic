/* Classic shell binding; the shell supplies its current destination without copying state. */
window.wbExportBindings = function (options) {
  var exportSpace = document.getElementById('exportSpace');
  var exportProgress = document.getElementById('exportProgress');
  var exportProgressBar = document.getElementById('exportProgressBar');
  var exportProgressText = document.getElementById('exportProgressText');
  var say = options.say;
  function progress(status, startedAt) {
    exportProgressBar.max = Math.max(1, status.total || 0);
    var elapsed = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
    if (status.total && !status.completed) {
      exportProgressBar.removeAttribute('value');
      exportProgressText.textContent =
        'Preparing capture… 0 of ' + status.total + ' complete · ' + elapsed + 's';
    } else {
      exportProgressBar.value = status.completed || 0;
      exportProgressText.textContent = status.total
        ? 'Captured ' + status.completed + ' of ' + status.total
        : 'Preparing download…';
    }
  }
  exportSpace.addEventListener('click', function () {
    var dialog = document.getElementById('exportDialog');
    Object.assign(dialog, options.destination());
    dialog.show();
  });
  document.getElementById('exportDialog').addEventListener('wb-export-request', function (event) {
    if (exportSpace.disabled) return;
    exportSpace.disabled = true;
    exportSpace.setAttribute('aria-busy', 'true');
    exportProgress.hidden = false;
    exportProgressBar.max = 1;
    exportProgressBar.value = 0;
    exportProgressText.textContent = 'Finding reference views…';
    window.dispatchEvent(new CustomEvent('wb-export-start'));
    say('Preparing export…');
    var finished = null;
    var startedAt = Date.now();
    options.review
      .start(event.detail)
      .then(function (job) {
        return options.review.poll(job, startedAt, progress);
      })
      .then(function (status) {
        finished = status;
        exportProgressText.textContent = 'Packaging download…';
        exportProgressText.textContent = 'Downloading export…';
        return options.review.download(status);
      })
      .then(function (name) {
        say(
          'Downloaded ' +
            name +
            (finished && finished.warnings
              ? ' with ' + finished.warnings + ' export warning(s).'
              : '.') +
            (finished && finished.warningMessages && finished.warningMessages.length
              ? ' ' + finished.warningMessages[0]
              : ''),
        );
      })
      .catch(function (error) {
        say(String(error.message || error));
      })
      .then(function () {
        exportProgress.hidden = true;
        exportSpace.disabled = false;
        exportSpace.removeAttribute('aria-busy');
        window.dispatchEvent(new CustomEvent('wb-export-end'));
      });
  });
};
