import type { DatatypeImplementation } from "@inkandswitch/patchwork-plugins";
import type { AllioFrameDoc } from "./types";

export const AllioFrameDatatype: DatatypeImplementation<AllioFrameDoc> = {
  init(doc: AllioFrameDoc) {
    doc["@patchwork"] = { type: "allio-frame" };
    doc.title = "Desktop Frame";
    doc.embeds = [];
  },
  getTitle(doc: AllioFrameDoc) {
    return doc.title || "Desktop Frame";
  },
  setTitle(doc: AllioFrameDoc, title: string) {
    doc.title = title.trim();
  },
};
