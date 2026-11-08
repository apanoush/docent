/** Native messaging connection to docent-native-host. */

export const HOST_NAME = "org.docent.host";

/**
 * Start a `run` request.
 *
 * The port stays open for the whole request. Each host message is passed to
 * `onMessage`. Disconnecting the port stops the host and its child process.
 */
export function runNative(cmd, args, request, onMessage) {
  const port = browser.runtime.connectNative(HOST_NAME);
  port.onMessage.addListener((message) => onMessage(message));
  port.onDisconnect.addListener(() => {
    const error = browser.runtime.lastError;
    onMessage({
      type: "disconnect",
      message: error ? error.message : "",
    });
  });
  port.postMessage({ type: "run", cmd, args, request });
  return port;
}

/** Run a short-lived command and return its output. */
export async function execNative(cmd, args) {
  return browser.runtime.sendNativeMessage(HOST_NAME, {
    type: "exec",
    cmd,
    args,
  });
}

/** Read the user's answer.css through the host. */
export async function cssNative(cmd, args) {
  return browser.runtime.sendNativeMessage(HOST_NAME, {
    type: "css",
    cmd,
    args,
  });
}
