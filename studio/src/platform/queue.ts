export class Queue {
  pending = Promise.resolve();
  run<T>(operation: () => T | Promise<T>): Promise<T> {
    const result = this.pending.then(operation);
    this.pending = result.then(
      () => {},
      () => {},
    );
    return result;
  }
}
