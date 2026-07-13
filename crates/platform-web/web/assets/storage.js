const DB_NAME = "wer-vault-v1";
const STORE = "records";

export const storageState = {
  mode: "memory",
  available: false,
  failures: 0,
};

export const openVault = () =>
  new Promise((resolve) => {
    if (!("indexedDB" in window)) {
      storageState.mode = "memory";
      resolve(storageState);
      return;
    }
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => {
      request.result.close();
      storageState.mode = "indexeddb";
      storageState.available = true;
      resolve(storageState);
    };
    request.onerror = () => {
      storageState.failures += 1;
      resolve(storageState);
    };
    request.onblocked = () => {
      storageState.failures += 1;
      resolve(storageState);
    };
  });

export const exportSnapshot = (snapshot) => {
  const blob = new Blob([JSON.stringify(snapshot, null, 2)], { type: "application/json" });
  return URL.createObjectURL(blob);
};
