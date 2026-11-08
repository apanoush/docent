/** Build the JSON request docent reads on stdin. */

/**
 * Build a request object containing only fields the CLI knows about.
 *
 * Offsets are omitted. DOM ranges and `innerText` use different coordinate
 * spaces, so the CLI's first-occurrence fallback is more reliable than a
 * guessed UTF-8 byte offset.
 */
export function buildRequest(capture, question = null) {
  const request = {
    selection: capture.selection ?? "",
    document: capture.document ?? "",
  };
  if (capture.file) {
    request.file = capture.file;
  }
  if (typeof question === "string" && question.trim()) {
    request.question = question;
  }
  return request;
}

/** Remove credentials, query parameters, and fragments from a page URL. */
export function cleanUrl(url) {
  if (!url) {
    return "";
  }
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return url;
  }
  if (!parsed.protocol) {
    return url;
  }
  parsed.username = "";
  parsed.password = "";
  parsed.search = "";
  parsed.hash = "";
  return parsed.toString();
}

/** Build the file label shown to the model. */
export function fileLabel(url, title) {
  const clean = cleanUrl(url);
  if (title && clean) {
    return `${title} (${clean})`;
  }
  return clean || title || "";
}
