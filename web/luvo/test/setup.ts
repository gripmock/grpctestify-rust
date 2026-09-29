(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const proto = globalThis.HTMLDialogElement?.prototype;
if (proto && !proto.showModal) {
  proto.showModal = function (this: HTMLDialogElement) { this.open = true; };
  proto.show = function (this: HTMLDialogElement) { this.open = true; };
  proto.close = function (this: HTMLDialogElement, returnValue?: string) {
    this.open = false;
    if (returnValue !== undefined) this.returnValue = returnValue;
    this.dispatchEvent(new Event('close'));
  };
}

/* jsdom has no layout, so it has no `scrollIntoView` — a component that keeps
   the active item in view crashed the render rather than the assertion. */
if (globalThis.Element && !globalThis.Element.prototype.scrollIntoView) {
  globalThis.Element.prototype.scrollIntoView = function () {};
}

/* Same for `ResizeObserver`: a strip that measures itself to decide whether it
   can scroll has nothing to measure here, and must still render. */
if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}

/* jsdom has no `matchMedia`, so a component that asks whether the window is
   wide enough for a layout crashed the render rather than the assertion. The
   stub answers "wide enough", which is the shape the tests are written for. */
if (globalThis.window && !globalThis.window.matchMedia) {
  globalThis.window.matchMedia = ((query: string) => ({
    matches: true,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
