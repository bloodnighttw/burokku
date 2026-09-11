export type BurokkuTagName = "div" | "flex" | "grid" | "text" | "window";

export interface BurokkuEvent {
  readonly type: string;
  readonly target: BurokkuNode;
  readonly currentTarget: BurokkuNode | null;
  readonly bubbles: boolean;
  readonly cancelable: boolean;
  readonly defaultPrevented: boolean;

  preventDefault(): void;
  stopPropagation(): void;
  stopImmediatePropagation(): void;
}

export interface BurokkuMouseEvent<Type extends string = string> extends BurokkuEvent {
  readonly type: Type;
  readonly clientX: number;
  readonly clientY: number;
  /** Changed button: primary=0, middle=1, secondary=2; 0 for movement and hover. */
  readonly button: number;
  /** Pressed-button bitmask: primary=1, secondary=2, middle=4. */
  readonly buttons: number;
  readonly relatedTarget: BurokkuNode | null;
}

export interface BurokkuClickEvent extends BurokkuMouseEvent<"click"> {}

export interface BurokkuWheelEvent extends BurokkuMouseEvent<"wheel"> {
  readonly deltaX: number;
  readonly deltaY: number;
  /** 0 for pixels, 1 for lines. */
  readonly deltaMode: 0 | 1;
}

export interface BurokkuPointerEvent<Type extends string = string> extends BurokkuMouseEvent<Type> {
  readonly pointerId: number;
  readonly pointerType: string;
  readonly isPrimary: boolean;
}

export interface BurokkuKeyboardEvent<Type extends string = string> extends BurokkuEvent {
  readonly type: Type;
  readonly key: string;
  /** Platform virtual key code. */
  readonly keyCode: number;
  readonly repeat: boolean;
  readonly shiftKey: boolean;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
}

export interface BurokkuEventMap {
  click: BurokkuClickEvent;
  keydown: BurokkuKeyboardEvent<"keydown">;
  keyup: BurokkuKeyboardEvent<"keyup">;
  pointerdown: BurokkuPointerEvent<"pointerdown">;
  pointerup: BurokkuPointerEvent<"pointerup">;
  pointermove: BurokkuPointerEvent<"pointermove">;
  pointerenter: BurokkuPointerEvent<"pointerenter">;
  pointerleave: BurokkuPointerEvent<"pointerleave">;
  pointercancel: BurokkuPointerEvent<"pointercancel">;
  gotpointercapture: BurokkuPointerEvent<"gotpointercapture">;
  lostpointercapture: BurokkuPointerEvent<"lostpointercapture">;
  wheel: BurokkuWheelEvent;
}

export type BurokkuEventListener<Event extends BurokkuEvent = BurokkuEvent> = (
  this: BurokkuNode,
  event: Event,
) => void;

/** Shared behavior implemented by every Burokku-native node wrapper. */
export interface Node<AllowedChild = never> {
  readonly parentNode: BurokkuNode | null;
  readonly childNodes: readonly AllowedChild[];
  readonly firstChild: AllowedChild | null;
  readonly lastChild: AllowedChild | null;
  readonly nextSibling: BurokkuNode | null;
  readonly previousSibling: BurokkuNode | null;
  readonly isConnected: boolean;
  readonly textContent: string;
  readonly nodeValue: string | null;

  appendChild<Child extends AllowedChild>(child: Child): Child;
  insertBefore<Child extends AllowedChild>(
    child: Child,
    reference: AllowedChild | null,
  ): Child;
  removeChild<Child extends AllowedChild>(child: Child): Child;
  replaceChild<OldChild extends AllowedChild>(
    newChild: AllowedChild,
    oldChild: OldChild,
  ): OldChild;
  contains(other: BurokkuNode): boolean;
  addEventListener<Type extends keyof BurokkuEventMap>(
    type: Type,
    callback: BurokkuEventListener<BurokkuEventMap[Type]>,
  ): void;
  addEventListener(type: string, callback: BurokkuEventListener): void;
  removeEventListener<Type extends keyof BurokkuEventMap>(
    type: Type,
    callback: BurokkuEventListener<BurokkuEventMap[Type]>,
  ): void;
  removeEventListener(type: string, callback: BurokkuEventListener): void;
}

/** The permanent, host-created application mount root. */
export interface AppNode extends Node<WindowElement> {
  createElement<Tag extends BurokkuTagName>(tag: Tag): BurokkuElement<Tag>;
  createTextNode(data: string): TextNode;
}

/** A raw DOM text node, distinct from the styled `text` element. */
export interface TextNode extends Node<never> {
  data: string;
  textContent: string;
  nodeValue: string;
}

/** The native style declaration exposed by a Burokku element. */
export interface BurokkuStyleDeclaration {
  supportsProperty(name: string): boolean;
  setProperty(name: string, value: string): void;
  removeProperty(name: string): void;
}

/** Last successfully presented border box in logical pixels. */
export interface BurokkuDOMRectReadOnly {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
  readonly left: number;
}

/** Shared behavior for script-creatable Burokku elements. */
export interface Element<
  Tag extends BurokkuTagName = BurokkuTagName,
  AllowedChild = BurokkuContainerChild,
> extends Node<AllowedChild> {
  readonly localName: Tag;
  readonly style: BurokkuStyleDeclaration;

  setPointerCapture(pointerId: number): void;
  releasePointerCapture(pointerId: number): void;
  hasPointerCapture(pointerId: number): boolean;

  getBoundingClientRect(): BurokkuDOMRectReadOnly | null;
  getAttribute(name: string): string | null;
  hasAttribute(name: string): boolean;
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
}

export interface DivElement extends Element<"div"> {}
export interface FlexElement extends Element<"flex"> {}
export interface GridElement extends Element<"grid"> {}
export interface TextElement extends Element<"text", BurokkuTextChild> {
  textContent: string;
}
export interface WindowElement extends Element<"window"> {}

/** Element children accepted by window and ordinary layout containers. */
export type BurokkuContainerChild =
  | DivElement
  | FlexElement
  | GridElement
  | TextElement;

/** Children accepted by a styled text element. */
export type BurokkuTextChild = TextNode | TextElement;

/** Any node exposed by the Burokku-native facade. */
export type BurokkuNode = AppNode | TextNode | BurokkuElement;

export interface BurokkuElementTagNameMap {
  div: DivElement;
  flex: FlexElement;
  grid: GridElement;
  text: TextElement;
  window: WindowElement;
}

export type BurokkuElement<
  Tag extends BurokkuTagName = BurokkuTagName,
> = BurokkuElementTagNameMap[Tag];

/** An immutable layout measurement in logical pixels, which may precede painting. */
export interface BurokkuResizeObserverEntry {
  readonly target: BurokkuElement;
  readonly size: {
    readonly width: number;
    readonly height: number;
  };
}

export type BurokkuResizeObserverCallback = (
  this: BurokkuResizeObserver,
  entries: readonly BurokkuResizeObserverEntry[],
  observer: BurokkuResizeObserver,
) => void;

/** Observes native layout width/height. Box-selection options are not supported yet. */
export interface BurokkuResizeObserver {
  observe(target: BurokkuElement): void;
  unobserve(target: BurokkuElement): void;
  disconnect(): void;
}

export type BurokkuDimension = "auto" | `${number}px` | `${number}%`;
export type BurokkuLength = `${number}px` | `${number}%`;
/** A CSS hexadecimal color: #rgb, #rgba, #rrggbb, or #rrggbbaa. */
export type BurokkuColor = `#${string}`;

export type BurokkuItemStyle = Partial<{
  flexBasis: BurokkuDimension;
  flexGrow: number;
  flexShrink: number;
  alignSelf: "auto" | "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  justifySelf: "auto" | "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  gridRow: string;
  gridRowStart: string;
  gridRowEnd: string;
  gridColumn: string;
  gridColumnStart: string;
  gridColumnEnd: string;
}>;

/** Box and item styles shared by div, flex, grid, and text elements. */
export type BurokkuCommonStyle = BurokkuItemStyle & Partial<{
  width: BurokkuDimension;
  height: BurokkuDimension;
  padding: BurokkuLength;
  margin: BurokkuLength;
  backgroundColor: BurokkuColor;
}>;

type BurokkuFlexContainerStyle = Partial<{
  flexDirection: "row" | "row-reverse" | "column" | "column-reverse";
  flexWrap: "nowrap" | "wrap" | "wrap-reverse";
  gap: BurokkuLength;
  columnGap: BurokkuLength;
  rowGap: BurokkuLength;
  alignContent: "start" | "end" | "flex-start" | "flex-end" | "center" | "stretch" | "space-between" | "space-around" | "space-evenly";
  alignItems: "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  justifyContent: "start" | "end" | "flex-start" | "flex-end" | "center" | "stretch" | "space-between" | "space-around" | "space-evenly";
}>;

type BurokkuGridContainerStyle = Partial<{
  gap: BurokkuLength;
  columnGap: BurokkuLength;
  rowGap: BurokkuLength;
  alignContent: "start" | "end" | "flex-start" | "flex-end" | "center" | "stretch" | "space-between" | "space-around" | "space-evenly";
  alignItems: "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  justifyContent: "start" | "end" | "flex-start" | "flex-end" | "center" | "stretch" | "space-between" | "space-around" | "space-evenly";
  justifyItems: "start" | "end" | "flex-start" | "flex-end" | "center" | "baseline" | "stretch";
  gridAutoFlow: "row" | "column" | "row dense" | "column dense" | "dense";
}>;

type BurokkuTypographyStyle = Partial<{
  fontFamily: string;
  fontSize: `${number}px`;
  fontWeight: number | "normal" | "bold";
  color: BurokkuColor;
  lineHeight: "normal" | number | `${number}px`;
  textWrap: "wrap" | "nowrap";
}>;

type BurokkuWindowStyleProperties = Partial<{
  width: BurokkuDimension;
  height: BurokkuDimension;
  backgroundColor: BurokkuColor;
}>;

type BurokkuAllStyle = BurokkuCommonStyle &
  BurokkuFlexContainerStyle &
  BurokkuGridContainerStyle &
  BurokkuTypographyStyle;

type BurokkuStyleFor<Style> = Style & Partial<
  Record<Exclude<keyof BurokkuAllStyle, keyof Style>, never>
>;

export type BurokkuDivStyle = BurokkuStyleFor<BurokkuCommonStyle>;
export type BurokkuFlexStyle = BurokkuStyleFor<
  BurokkuCommonStyle & BurokkuFlexContainerStyle
>;
export type BurokkuGridStyle = BurokkuStyleFor<
  BurokkuCommonStyle & BurokkuGridContainerStyle
>;
export type BurokkuTextStyle = BurokkuStyleFor<
  BurokkuCommonStyle & BurokkuTypographyStyle
>;
export type BurokkuWindowStyle = BurokkuStyleFor<BurokkuWindowStyleProperties>;

/** Native styles supported by each Burokku element tag. */
export interface BurokkuElementStyleMap {
  div: BurokkuDivStyle;
  flex: BurokkuFlexStyle;
  grid: BurokkuGridStyle;
  text: BurokkuTextStyle;
  window: BurokkuWindowStyle;
}

/** Native styles supported by a Burokku element tag. */
export type BurokkuStyle<Tag extends BurokkuTagName = BurokkuTagName> =
  BurokkuElementStyleMap[Tag];

/** Apply native styles supported by the element's tag. */
export function setStyles<ElementType extends BurokkuElement>(
  element: ElementType,
  styles: Readonly<BurokkuStyle<NoInfer<ElementType["localName"]>>>,
): void {
  for (const [name, value] of Object.entries(styles)) {
    if (value === undefined) continue;
    const nativeName = name.replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`);
    element.style.setProperty(nativeName, String(value));
  }
}

declare global {
  /** The permanent Burokku application mount root. */
  var app: AppNode;

  /** Installed automatically by Burokku after its DOM bindings. */
  var ResizeObserver: {
    readonly prototype: BurokkuResizeObserver;
    new(callback: BurokkuResizeObserverCallback): BurokkuResizeObserver;
  };
}
