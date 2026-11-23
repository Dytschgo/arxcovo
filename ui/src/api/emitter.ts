export class Emitter<A extends unknown[]> {
  private listeners = new Set<(...args: A) => void>();

  on(listener: (...args: A) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  emit(...args: A): void {
    for (const listener of [...this.listeners]) listener(...args);
  }

  clear(): void {
    this.listeners.clear();
  }
}
