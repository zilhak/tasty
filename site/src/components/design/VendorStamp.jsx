import React from "react";
import { VENDOR_STAMP } from "../../gallery/vendor-stamp.js";

/** Show the last vendor change, excluding README edits; omit unavailable Git data. */

const COPY = {
  en: {
    lead: "This page uses a saved copy of the design system, last changed on ",
    tail: ". It may differ from the current design source.",
  },
  ko: {
    lead: "이 페이지는 저장소에 보관한 디자인 사본을 사용합니다. 사본의 마지막 수정일은 ",
    tail: "입니다. 현재 디자인 원본과 다를 수 있습니다.",
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
