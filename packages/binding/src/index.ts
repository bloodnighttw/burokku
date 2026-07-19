import { add as rustAdd, greet as rustGreet } from "../wasm/burokku.js";

/** Return a greeting produced by the Rust library. */
export async function greet(name: string): Promise<string> {
  return rustGreet(name);
}

/** Add two numbers using the Rust library. */
export async function add(left: number, right: number): Promise<number> {
  return rustAdd(left, right);
}
