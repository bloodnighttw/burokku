import type {
  BurokkuClickEvent,
  BurokkuEventListener,
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
