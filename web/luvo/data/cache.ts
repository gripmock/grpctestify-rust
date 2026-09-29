
export class LRUCache<K, V> {
  private readonly cap: number;
  private readonly map = new Map<K, V>();

  constructor(capacity: number) {
    if (capacity < 1) throw new Error('capacity must be >= 1');
    this.cap = capacity;
  }

  
  get(key: K): V | undefined {
    if (!this.map.has(key)) return undefined;
    const value = this.map.get(key)!;
    
    this.map.delete(key);
    this.map.set(key, value);
    return value;
  }

  
  put(key: K, value: V): void {
    
    if (this.map.has(key)) {
      this.map.delete(key);
    }
    this.map.set(key, value);
    
    while (this.map.size > this.cap) {
      const lruKey = this.map.keys().next().value;
      if (lruKey !== undefined) this.map.delete(lruKey);
    }
  }

  
  delete(key: K): boolean {
    return this.map.delete(key);
  }

  
  clear(): void {
    this.map.clear();
  }

  
  get size(): number {
    return this.map.size;
  }

  
  entries(): [K, V][] {
    return [...this.map.entries()].reverse();
  }

  
  values(): V[] {
    return [...this.map.values()].reverse();
  }
}
