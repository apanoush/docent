/** Capture the selection and rendered text of a browser tab. */

import { fileLabel } from "./request.js";

const CAPTURE_CODE = `
(() => {
  const root = document.body || document.documentElement;
  const selection = window.getSelection ? window.getSelection().toString() : "";
  return {
    selection: selection,
    document: root ? root.innerText : "",
    url: location.href,
    title: document.title || ""
  };
})()
`;

function text(value) {
  return typeof value === "string" ? value : "";
}

/**
 * Run the capture snippet in `tabId` and return a request-ready object.
 *
 * `activeTab` covers this call. Restricted pages, such as reader view, the
 * PDF viewer, and browser UI, reject the injection.
 */
export async function captureTab(tabId) {
  let results;
  try {
    results = await browser.tabs.executeScript(tabId, {
      code: CAPTURE_CODE,
      runAt: "document_idle",
    });
  } catch (error) {
    throw new Error(`could not read this page: ${error.message}`);
  }

  const value = results && results[0];
  if (!value || typeof value !== "object") {
    throw new Error("could not read this page");
  }

  return {
    selection: text(value.selection),
    document: text(value.document),
    file: fileLabel(text(value.url), text(value.title)),
  };
}
