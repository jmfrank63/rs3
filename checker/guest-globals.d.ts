declare var console: { log(...args: unknown[]): void; info(...args: unknown[]): void; warn(...args: unknown[]): void; error(...args: unknown[]): void; debug(...args: unknown[]): void; };
declare function test(name: string, fn: () => void): void;
declare function assert(cond: unknown, msg?: string): void;
interface Expectation {
  toEqual(expected: unknown): void;
  toBeTrue(msg?: string): void;
  toBeCloseTo(expected: number, eps?: number): void;
}
declare function expect(actual: unknown): Expectation;