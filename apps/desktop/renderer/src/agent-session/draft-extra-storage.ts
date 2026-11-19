const STORE = 'extra';
let database: Promise<IDBDatabase> | undefined;

function open() {
  if (!database) {
    database = new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open('aura-composer-drafts', 1);
      request.onupgradeneeded = () => request.result.createObjectStore(STORE);
      request.onsuccess = () => {
        request.result.onversionchange = () => {
          request.result.close();
          database = undefined;
        };
        resolve(request.result);
      };
      request.onerror = () => reject(request.error);
    }).catch((error: unknown) => {
      database = undefined;
      throw error;
    });
  }
  return database;
}

export async function readDraftExtra(key: string): Promise<unknown[] | undefined> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const request = db.transaction(STORE).objectStore(STORE).get(key);
    request.onsuccess = () => resolve(Array.isArray(request.result) ? request.result : undefined);
    request.onerror = () => reject(request.error);
  });
}

export async function saveDraftExtra(key: string, value: readonly unknown[]): Promise<void> {
  const db = await open();
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite', { durability: 'strict' });
    const store = tx.objectStore(STORE);
    if (value.length) store.put(value, key);
    else store.delete(key);
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error);
  });
}
