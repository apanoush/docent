import assert from "node:assert/strict";
import test from "node:test";

import {
  ProfileError,
  hasRequirement,
  parseProfiles,
} from "../../plugins/firefox/lib/profiles.js";

test("parses profiles", () => {
  assert.deepEqual(
    parseProfiles({
      profiles: [
        {
          name: "explain",
          description: "Explain a passage",
          requires: ["selection"],
        },
        { name: "summarize", requires: [] },
        { name: "ask" },
      ],
    }),
    [
      {
        name: "explain",
        description: "Explain a passage",
        requires: ["selection"],
      },
      { name: "summarize", description: null, requires: [] },
      { name: "ask", description: null, requires: [] },
    ],
  );
});

test("rejects a non-object payload", () => {
  assert.throws(() => parseProfiles([]), ProfileError);
  assert.throws(() => parseProfiles(null), ProfileError);
});

test("rejects a missing profile array", () => {
  assert.throws(() => parseProfiles({}), /missing profiles array/);
});

test("rejects invalid entries", () => {
  assert.throws(() => parseProfiles({ profiles: [{}] }), /invalid profile entry/);
  assert.throws(
    () => parseProfiles({ profiles: [{ name: "x", description: 1 }] }),
    /invalid description/,
  );
  assert.throws(
    () => parseProfiles({ profiles: [{ name: "x", requires: [1] }] }),
    /invalid requires/,
  );
});

test("checks requirements", () => {
  const profile = { name: "ask", requires: ["selection", "question"] };
  assert.equal(hasRequirement(profile, "selection"), true);
  assert.equal(hasRequirement(profile, "question"), true);
  assert.equal(hasRequirement(profile, "context"), false);
});
