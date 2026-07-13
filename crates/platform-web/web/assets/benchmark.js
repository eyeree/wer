export const runStartupBenchmark = () => {
  const start = performance.now();
  let hash = 0;
  for (let i = 0; i < 4096; i += 1) {
    hash = (hash * 1664525 + i + 1013904223) >>> 0;
  }
  return {
    ms: performance.now() - start,
    hash,
    hardwareConcurrency: navigator.hardwareConcurrency ?? 1,
    sharedMemory: typeof SharedArrayBuffer !== "undefined" && crossOriginIsolated,
  };
};
