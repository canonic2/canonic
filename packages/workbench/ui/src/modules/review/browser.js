import { requestJson } from '../../host/http.js';

/** Browser delivery resources belong to this adapter, never to a component. */
export function createBrowserReview(document) {
  const downloads = new Map();
  return {
    async save(blob) {
      const response = await fetch('/_workbench/shot?name=workbench-canvas', {
        method: 'POST',
        headers: { 'Content-Type': 'image/jpeg' },
        body: blob,
      });
      const saved = await response.json();
      if (!response.ok || typeof saved.file !== 'string')
        throw new Error(saved.error || 'Could not save screenshot.');
      return saved.file;
    },
    handoff: (payload) => requestJson('/_workbench/handoff', payload),
    download(blob) {
      const url = URL.createObjectURL(blob),
        link = document.createElement('a');
      link.href = url;
      link.download = 'workbench-canvas.jpg';
      try {
        link.click();
      } catch (error) {
        URL.revokeObjectURL(url);
        throw error;
      }
      downloads.set(
        url,
        setTimeout(() => {
          URL.revokeObjectURL(url);
          downloads.delete(url);
        }, 60000),
      );
    },
    dispose() {
      for (const [url, timer] of downloads) {
        clearTimeout(timer);
        URL.revokeObjectURL(url);
      }
      downloads.clear();
    },
  };
}
