import type {
  BurokkuElement,
  BurokkuResizeObserver,
  BurokkuResizeObserverCallback,
  BurokkuResizeObserverEntry,
} from "../src/index";

const callback: BurokkuResizeObserverCallback = function (entries, observer) {
  const current: BurokkuResizeObserver = this;
  const entry: BurokkuResizeObserverEntry = entries[0];
  const target: BurokkuElement = entry.target;
  const width: number = entry.size.width;
  const height: number = entry.size.height;
  observer.unobserve(target);
  void [current, width, height];

  // @ts-expect-error The delivered batch is immutable.
  entries.push(entry);
  // @ts-expect-error The target reference is immutable.
  entry.target = app.createElement("div");
  // @ts-expect-error The size record is immutable.
  entry.size = { width: 1, height: 1 };
  // @ts-expect-error Measured dimensions are immutable.
  entry.size.width = 1;
  // @ts-expect-error Separate border-box measurements are not exposed.
  entry.borderBoxSize;
};

const observer: BurokkuResizeObserver = new ResizeObserver(callback);
observer.observe(app.createElement("window"));
observer.observe(app.createElement("div"));
observer.observe(app.createElement("text"));
observer.disconnect();

// @ts-expect-error The app root is not an element target.
observer.observe(app);
// @ts-expect-error Raw text nodes are not element targets.
observer.observe(app.createTextNode("text"));
// @ts-expect-error unobserve also requires an element.
observer.unobserve(app);
// @ts-expect-error Box-selection options are not supported.
observer.observe(app.createElement("div"), { box: "border-box" });
// @ts-expect-error A callback is required.
new ResizeObserver();
// @ts-expect-error The callback must be callable.
new ResizeObserver(1);
// @ts-expect-error Manual record consumption is a native API, not a JS method.
observer.takeRecords();
