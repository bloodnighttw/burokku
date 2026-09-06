import type {
  BurokkuClickEvent,
  BurokkuEventListener,
  BurokkuMouseEvent,
  BurokkuWheelEvent,
  BurokkuNode,
  DivElement,
} from "../src/index";

declare const div: DivElement;

const clickListener: BurokkuEventListener<BurokkuClickEvent> = function (event) {
  const type: "click" = event.type;
  const target: BurokkuNode = event.target;
  const currentTarget: BurokkuNode | null = event.currentTarget;
  const listenerTarget: BurokkuNode = this;
  const clientX: number = event.clientX;
  const clientY: number = event.clientY;
  const button: number = event.button;
  event.preventDefault();
  event.stopPropagation();
  event.stopImmediatePropagation();
  void [type, target, currentTarget, listenerTarget, clientX, clientY, button];

  // @ts-expect-error Event coordinates are read-only.
  event.clientX = 0;
};

div.addEventListener("click", clickListener);
div.removeEventListener("click", clickListener);
div.addEventListener("click", event => {
  const click: BurokkuClickEvent = event;
  void click;
});
div.addEventListener("custom", event => {
  const type: string = event.type;
  void type;
  // @ts-expect-error Unknown events do not expose mouse coordinates.
  event.clientX;
});

// @ts-expect-error Click listeners receive BurokkuClickEvent.
div.addEventListener("click", (event: string) => void event);

for (const type of [
  "mousedown", "mouseup", "mousemove", "mouseover", "mouseout", "mouseenter", "mouseleave",
] as const) {
  const listener: BurokkuEventListener<BurokkuMouseEvent<typeof type>> = function (event) {
    const eventType: typeof type = event.type;
    const buttons: number = event.buttons;
    const relatedTarget: BurokkuNode | null = event.relatedTarget;
    const target: BurokkuNode = this;
    void [eventType, buttons, relatedTarget, target];
    // @ts-expect-error Button state is read-only.
    event.buttons = 0;
    // @ts-expect-error Related target is read-only.
    event.relatedTarget = null;
  };
  div.addEventListener(type, listener);
  div.removeEventListener(type, listener);
}

div.addEventListener("mousedown", event => {
  const type: "mousedown" = event.type;
  const x: number = event.clientX;
  void [type, x];
});
div.addEventListener("mouseup", event => {
  const type: "mouseup" = event.type;
  void type;
});
div.addEventListener("mousemove", event => {
  const type: "mousemove" = event.type;
  void type;
});
const wheelListener: BurokkuEventListener<BurokkuWheelEvent> = event => {
  const type: "wheel" = event.type;
  const deltaX: number = event.deltaX;
  const deltaY: number = event.deltaY;
  const deltaMode: 0 | 1 = event.deltaMode;
  void [type, deltaX, deltaY, deltaMode];
  // @ts-expect-error Wheel deltas are read-only.
  event.deltaY = 0;
};
div.addEventListener("wheel", wheelListener);
div.removeEventListener("wheel", wheelListener);

// @ts-expect-error Mouse listeners receive a mouse event.
div.addEventListener("mousedown", (event: string) => void event);
// @ts-expect-error A click-only listener cannot handle mouse movement.
div.addEventListener("mousemove", clickListener);

div.addEventListener("mouseenter", event => {
  const type: "mouseenter" = event.type;
  const related: BurokkuNode | null = event.relatedTarget;
  void [type, related];
});
div.addEventListener("mouseleave", event => {
  const type: "mouseleave" = event.type;
  void type;
});
div.addEventListener("mouseover", event => {
  const type: "mouseover" = event.type;
  void type;
});
div.addEventListener("mouseout", event => {
  const type: "mouseout" = event.type;
  void type;
});
// @ts-expect-error Hover events are not clicks.
div.addEventListener("mouseenter", clickListener);
