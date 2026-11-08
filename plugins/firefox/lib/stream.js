/** Parse newline-delimited JSON events from the docent CLI. */

export class StreamError extends Error {
  constructor(message) {
    super(message);
    this.name = "StreamError";
  }
}

/**
 * Parse one NDJSON line.
 *
 * Blank lines return null. Invalid JSON and non-object values throw, so a
 * broken stream is reported instead of silently dropped.
 */
export function parseEvent(line) {
  if (!line.trim()) {
    return null;
  }

  let value;
  try {
    value = JSON.parse(line);
  } catch (error) {
    throw new StreamError(`invalid stream event: ${error.message}`);
  }

  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new StreamError("invalid stream event: expected a JSON object");
  }
  return value;
}

/** Accumulate text and return every complete event. */
export class NdjsonParser {
  constructor() {
    this.pending = "";
  }

  feed(text) {
    if (!text) {
      return [];
    }
    this.pending += text;

    const events = [];
    let newline = this.pending.indexOf("\n");
    while (newline >= 0) {
      const line = this.pending.slice(0, newline);
      this.pending = this.pending.slice(newline + 1);
      const event = parseEvent(line);
      if (event !== null) {
        events.push(event);
      }
      newline = this.pending.indexOf("\n");
    }
    return events;
  }

  flush() {
    if (!this.pending) {
      return [];
    }
    const line = this.pending;
    this.pending = "";
    const event = parseEvent(line);
    return event === null ? [] : [event];
  }
}
