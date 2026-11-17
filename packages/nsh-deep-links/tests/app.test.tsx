import { expect, open, test } from "nsh/test";

test("the demo routes a delivered link and refuses an unclaimed scheme", async () => {
  const app = await open("../App.tsx", { clock: {} });
  await app.waitFor("text:nsh-deep-links");
  await app.press("id:open");
  await app.waitFor('text:custom Order {"id":42}');
  await app.press("id:unclaimed");
  await app.waitFor("text:ERR_DEEP_LINKS_DISALLOWED_SCHEME");
});

test("a_next_waiting_past_its_deadline_resolves_as_timeout", async () => {
  const app = await open("./fixtures/deadline.tsx", { clock: {} });
  const clock = app.clock;
  if (clock === null) throw new Error("the app runs on a manual clock");
  await app.press("id:wait");
  await clock.runFor(29_999);
  expect(app.texts()).toContain("waiting");
  await clock.runFor(1);
  await app.waitFor("text:ERR_DEEP_LINKS_TIMEOUT");
  // The gate is free again, and the next link is not lost.
  await app.press("id:deliver");
  await app.press("id:wait");
  await app.waitFor("text:link Chat");
});
