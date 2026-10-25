export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, Math.max(ms, 0)));

/** Rejects if the promise has not settled within `ms`; a late result is ignored. */
export function within<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Timed out')), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}
