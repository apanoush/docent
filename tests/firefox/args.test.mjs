import assert from "node:assert/strict";
import test from "node:test";

import { contextMode, profilesArgs, requestArgs } from "../../plugins/firefox/lib/args.js";

const settings = {
  cmd: "docent",
  configDir: "",
  defaultProfile: "explain",
  contextMode: "",
  targetLanguage: "",
};

test("loads profiles", () => {
  assert.deepEqual(profilesArgs(settings), ["profiles", "--json"]);
});

test("passes the config directory before the subcommand", () => {
  assert.deepEqual(profilesArgs({ ...settings, configDir: "/tmp/config" }), [
    "--config-dir",
    "/tmp/config",
    "profiles",
    "--json",
  ]);
});

test("builds a run request", () => {
  assert.deepEqual(requestArgs(settings, "explain"), [
    "request",
    "--profile",
    "explain",
    "--stream",
    "--html",
    "--source",
    "firefox",
  ]);
});

test("adds language and context overrides", () => {
  assert.deepEqual(
    requestArgs(settings, "translate", {
      targetLanguage: "French",
      contextMode: "full",
    }),
    [
      "request",
      "--profile",
      "translate",
      "--stream",
      "--html",
      "--target-language",
      "French",
      "--context-mode",
      "full",
      "--source",
      "firefox",
    ],
  );
});

test("uses settings defaults", () => {
  assert.deepEqual(
    requestArgs(
      { ...settings, targetLanguage: "German", contextMode: "none" },
      "translate",
    ),
    [
      "request",
      "--profile",
      "translate",
      "--stream",
      "--html",
      "--target-language",
      "German",
      "--context-mode",
      "none",
      "--source",
      "firefox",
    ],
  );
});

test("rejects an unknown context mode", () => {
  assert.equal(contextMode("everything"), "");
  assert.equal(contextMode("auto"), "auto");
  assert.deepEqual(
    requestArgs(settings, "explain", { contextMode: "everything" }),
    ["request", "--profile", "explain", "--stream", "--html", "--source", "firefox"],
  );
});
