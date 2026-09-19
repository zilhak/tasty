import React from "react";
import { VENDOR_STAMP } from "../../gallery/vendor-stamp.js";

/**
 * How old the vendored copy is, on every page that draws from it.
 *
 * The date is read from git at build time by `scripts/vendor-to-esm.mjs` and
 * never written here. A date typed into a source file is a second copy of the
 * fact, and it goes stale at exactly the moment it matters — it would say
 * "current" on the day the copy stops being current. When git cannot answer
 * the stamp is absent rather than guessed, so the page says nothing instead of
 * saying something false.
 *
 * The wording says "last changed", not "taken on". `site/vendor/` is not one
 * clean snapshot: it was imported once and then patched in places, so its
 * newest commit date is the date of the last patch, not of a whole receipt.
 * Calling it a snapshot would be the same kind of lie this stamp exists to
 * prevent.
 *
 * The sentence lives here rather than in each page, because it is one fact and
 * a second copy of it would drift. `site/src/styles/vendor-stamp.css` carries
 * its rule for the same reason — the gallery and the design section load
 * different stylesheets and would otherwise each hold a copy.
 */

const COPY = {
  en: {
    lead: "Built from a vendored copy of the design system. That copy is patched over time, not taken whole; its last change was on ",
    tail: ", and nothing decided after that date has reached this page.",
  },
  ko: {
    lead: "이 페이지는 디자인 시스템의 vendored 사본으로 그립니다. 그 사본은 한 시점에 통째로 받아온 것이 아니라 그때그때 기워 온 것이고, 마지막으로 바뀐 날은 ",
    tail: " 입니다. 그 뒤에 내려진 결정은 아직 이 페이지에 반영되지 않았습니다.",
  },
};

export function VendorStamp({ lang = "en" }) {
  if (!VENDOR_STAMP) return null;
  const c = COPY[lang] ?? COPY.en;
  return (
    <p className="vendor-stamp">
      {c.lead}
      <time dateTime={VENDOR_STAMP.date}>{VENDOR_STAMP.date}</time>
      {c.tail}
    </p>
  );
}
