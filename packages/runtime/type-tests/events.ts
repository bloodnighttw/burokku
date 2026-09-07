import type {
  BurokkuClickEvent,
  BurokkuKeyboardEvent,
  BurokkuEventListener,
  BurokkuPointerEvent,
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

for (const type of ["keydown", "keyup"] as const) {
  const listener: BurokkuEventListener<BurokkuKeyboardEvent<typeof type>> = event => {
    const eventType: typeof type = event.type;
    const key: string = event.key;
    const keyCode: number = event.keyCode;
    const repeat: boolean = event.repeat;
    const modifiers: boolean[] = [event.shiftKey, event.ctrlKey, event.altKey, event.metaKey];
    void [eventType, key, keyCode, repeat, modifiers];
    // @ts-expect-error Keyboard payloads are read-only.
    event.key = "x";
  };
  div.addEventListener(type, listener);
  div.removeEventListener(type, listener);
}


for (const type of [
  "pointerdown", "pointerup", "pointermove", "pointerenter", "pointerleave", "pointercancel",
  "gotpointercapture", "lostpointercapture",
] as const) {
  const listener: BurokkuEventListener<BurokkuPointerEvent<typeof type>> = event => {
    const eventType: typeof type = event.type;
    const pointerId: number = event.pointerId;
    const pointerType: string = event.pointerType;
    const isPrimary: boolean = event.isPrimary;
    const clientX: number = event.clientX;
    void [eventType, pointerId, pointerType, isPrimary, clientX];
    // @ts-expect-error Pointer identity is read-only.
    event.pointerId = 2;
  };
  div.addEventListener(type, listener);
  div.removeEventListener(type, listener);
}

div.setPointerCapture(1);
div.releasePointerCapture(1);
const hasPointerCapture: boolean = div.hasPointerCapture(1);
void hasPointerCapture;

div.addEventListener("mousedown", event => {
  const type: string = event.type;
  void type;
  // @ts-expect-error Mouse events are not part of the native event map.
  event.clientX;
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
