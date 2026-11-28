async (includeOpfs) => {
  let stage = 'cookie-and-local-storage';
  try {
  document.cookie = 'Acme-associated=Acme-private-cookie; Path=/; SameSite=Lax';
  localStorage.setItem('Acme-account', 'Acme-private-local');
  sessionStorage.setItem('Acme-session', 'Acme-private-session');
  stage = 'indexeddb-open';
  const database = await new Promise((resolve, reject) => {
    const request = indexedDB.open('Acme-associated-db', 1);
    request.onupgradeneeded = () => {
      const store = request.result.createObjectStore('records', {keyPath: 'id'});
      store.createIndex('byName', 'name');
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  try {
    stage = 'indexeddb-write';
    await new Promise((resolve, reject) => {
      const transaction = database.transaction('records', 'readwrite');
      transaction.objectStore('records').put({id: 1, name: 'Acme-private-record'});
      transaction.oncomplete = resolve;
      transaction.onerror = () => reject(transaction.error);
      transaction.onabort = () => reject(transaction.error);
    });
  } finally { database.close(); }
  const opfsAvailable = typeof navigator.storage?.getDirectory === 'function';
  if (includeOpfs && opfsAvailable) {
    stage = 'opfs-root';
    const root = await navigator.storage.getDirectory();
    stage = 'opfs-file';
    const file = await root.getFileHandle('Acme-private.txt', {create: true});
    const writer = await file.createWritable();
    await writer.write('Acme-private-file');
    await writer.close();
  }
  return opfsAvailable;
  } catch (error) { throw new Error(`Acme state fixture ${stage}: ${error.name}: ${error.message}`); }
}
