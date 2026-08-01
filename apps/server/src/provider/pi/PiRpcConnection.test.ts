import { describe, expect, it } from "@effect/vitest";

import { splitJsonlBuffer } from "./PiRpcConnection.ts";

describe("splitJsonlBuffer", () => {
  it("returns complete records and keeps the unterminated tail", () => {
    expect(splitJsonlBuffer('{"a":1}\n{"b":2}\n{"c"')).toEqual({
      lines: ['{"a":1}', '{"b":2}'],
      remainder: '{"c"',
    });
  });

  it("strips a single trailing CR so CRLF output parses", () => {
    expect(splitJsonlBuffer('{"a":1}\r\n').lines).toEqual(['{"a":1}']);
  });

  it("does not split on a bare CR, which is legal inside a JSON string", () => {
    // A generic line reader would break this record in half and lose the event.
    const { lines, remainder } = splitJsonlBuffer('{"text":"a\rb"}\n');
    expect(lines).toEqual(['{"text":"a\rb"}']);
    expect(remainder).toBe("");
  });

  it("does not split on Unicode line separators", () => {
    const line = '{"text":"a\u2028b\u2029c"}';
    expect(splitJsonlBuffer(`${line}\n`).lines).toEqual([line]);
  });

  it("treats an empty buffer and a buffer with no newline as all-remainder", () => {
    expect(splitJsonlBuffer("")).toEqual({ lines: [], remainder: "" });
    expect(splitJsonlBuffer("partial")).toEqual({ lines: [], remainder: "partial" });
  });

  it("preserves empty records so line counting stays honest", () => {
    expect(splitJsonlBuffer("\n\n").lines).toEqual(["", ""]);
  });
});
