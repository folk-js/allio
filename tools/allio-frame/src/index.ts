import type { Plugin } from "@inkandswitch/patchwork-plugins";

export const plugins: Plugin<any>[] = [
  {
    type: "patchwork:datatype",
    id: "allio-frame",
    name: "Desktop Frame",
    icon: "LayoutDashboard",
    async load() {
      const { AllioFrameDatatype } = await import("./datatype");
      return AllioFrameDatatype;
    },
  },
  {
    type: "patchwork:tool",
    id: "allio-frame",
    name: "Desktop Frame",
    icon: "LayoutDashboard",
    supportedDatatypes: ["allio-frame"],
    async load() {
      const { renderAllioFrame } = await import("./frame");
      return renderAllioFrame;
    },
  },
  {
    type: "patchwork:datatype",
    id: "allio-todo",
    name: "Todo",
    icon: "ListChecks",
    async load() {
      const { TodoDatatype } = await import("./todo");
      return TodoDatatype;
    },
  },
  {
    type: "patchwork:tool",
    id: "allio-todo",
    name: "Todo",
    icon: "ListChecks",
    supportedDatatypes: ["allio-todo"],
    async load() {
      const { renderTodo } = await import("./todo");
      return renderTodo;
    },
  },
];
