/**
 * Reading a UI action's arguments. The model's JSON arrives unchecked, and an
 * error message is the only thing it learns from, so every rejection names
 * the key and the shape that was wanted.
 */

/** A UI action the window will not perform, with the reason the model reads. */
export class UiRefusal extends Error {}

export function refuse(message: string): never {
  throw new UiRefusal(message);
}

export interface ArgReader {
  str(key: string): string;
  optStr(key: string): string | undefined;
  optInt(key: string, min?: number): number | undefined;
  optBool(key: string): boolean | undefined;
  oneOf<T extends string>(key: string, allowed: readonly T[]): T;
  optOneOf<T extends string>(key: string, allowed: readonly T[]): T | undefined;
}

export function readArgs(tool: string, args: Record<string, unknown>): ArgReader {
  const wrong = (key: string, wanted: string) => refuse(`${tool}: ${key} must be ${wanted}`);
  const optStr = (key: string) => {
    const v = args[key];
    if (v === undefined || v === null) return undefined;
    if (typeof v !== "string" || v.trim() === "") return wrong(key, "a non-empty string");
    return v;
  };
  const optOneOf = <T extends string>(key: string, allowed: readonly T[]) => {
    const v = args[key];
    if (v === undefined || v === null) return undefined;
    if (typeof v !== "string" || !allowed.includes(v as T))
      return wrong(key, `one of ${allowed.join(", ")}`);
    return v as T;
  };
  return {
    optStr,
    str: (key) => optStr(key) ?? wrong(key, "a non-empty string"),
    optInt: (key, min = 1) => {
      const v = args[key];
      if (v === undefined || v === null) return undefined;
      if (typeof v !== "number" || !Number.isInteger(v) || v < min)
        return wrong(key, `an integer ≥ ${min}`);
      return v;
    },
    optBool: (key) => {
      const v = args[key];
      if (v === undefined || v === null) return undefined;
      if (typeof v !== "boolean") return wrong(key, "true or false");
      return v;
    },
    optOneOf,
    oneOf: (key, allowed) => optOneOf(key, allowed) ?? wrong(key, `one of ${allowed.join(", ")}`),
  };
}
